//! Structural HTML extraction, shared by [`crate::url`]'s fetched pages
//! (native `.html`/`.htm` files in `raw/` still go through
//! `PlainExtractor`'s simpler regex tag-stripper for now — see that
//! module's docs — though this is the natural place to point them too,
//! later).
//!
//! Uses `scraper` (Servo's `html5ever` + `selectors`, safe pure Rust) to
//! walk the real DOM instead of stripping tags with a regex: `<h1>`-
//! `<h6>` become headings with real depth, `<p>`/`<li>`/`<blockquote>`
//! become body text, `<pre>` becomes code, `<table>` becomes a table
//! block — the same structural shape `markdown.rs` produces, so a
//! fetched web page is exactly as indexable as a hand-written note.

use crate::structured::{synthetic_bbox, StructuredBlock};
use brain_core::BlockKind;
use scraper::{ElementRef, Html, Node};

/// Tags whose entire subtree is never indexed — not content, or not
/// content in any sense this graph should store (a page's inline
/// JavaScript/CSS showing up as "prose" would be pure noise, worse than
/// useless for search relevance).
const EXCLUDED_ANCESTORS: &[&str] = &["script", "style", "noscript"];

/// Parses `html`, returning its `<title>` (if any, whitespace-normalized)
/// and every recognized content block in document order.
pub fn html_to_blocks(html: &str) -> (Option<String>, Vec<StructuredBlock>) {
    let doc = Html::parse_document(html);

    let title = doc
        .root_element()
        .descendent_elements()
        .find(|el| el.value().name() == "title")
        .map(|el| normalize_ws(&el.text().collect::<Vec<_>>().join(" ")))
        .filter(|s| !s.is_empty());

    let mut blocks = Vec::new();
    let mut ord: u32 = 0;
    for el in doc.root_element().descendent_elements() {
        let Some((kind, heading_level)) = recognized_kind(el.value().name()) else { continue };
        // Skip anything inside script/style, and anything whose parent
        // is *also* a recognized block tag (e.g. a stray `<p>` nested in
        // another `<p>`, or a `<code>` inside a `<pre>`) — its text was
        // already captured by that ancestor's own `.text()` call, so
        // capturing it again here would duplicate it.
        if has_ancestor(&el, EXCLUDED_ANCESTORS) || has_recognized_ancestor(&el) {
            continue;
        }
        let text = normalize_ws(&el.text().collect::<Vec<_>>().join(" "));
        if text.is_empty() {
            continue;
        }
        blocks.push(StructuredBlock { col: 0, ord, bbox: synthetic_bbox(ord), kind, text, heading_level });
        ord += 1;
    }
    (title, blocks)
}

fn recognized_kind(tag: &str) -> Option<(BlockKind, Option<u8>)> {
    match tag {
        "h1" => Some((BlockKind::Heading, Some(1))),
        "h2" => Some((BlockKind::Heading, Some(2))),
        "h3" => Some((BlockKind::Heading, Some(3))),
        "h4" => Some((BlockKind::Heading, Some(4))),
        "h5" => Some((BlockKind::Heading, Some(5))),
        "h6" => Some((BlockKind::Heading, Some(6))),
        "p" | "li" | "blockquote" => Some((BlockKind::Body, None)),
        "pre" => Some((BlockKind::Code, None)),
        "table" => Some((BlockKind::Table, None)),
        _ => None,
    }
}

fn has_ancestor(el: &ElementRef, tags: &[&str]) -> bool {
    el.ancestors().any(|a| matches!(a.value(), Node::Element(e) if tags.contains(&e.name())))
}

fn has_recognized_ancestor(el: &ElementRef) -> bool {
    el.ancestors().any(|a| matches!(a.value(), Node::Element(e) if recognized_kind(e.name()).is_some()))
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_and_paragraphs_become_blocks_with_real_depth() {
        let html = "<html><head><title>My Page</title></head><body><h1>Title</h1><p>First para.</p><h2>Sub</h2><p>Second para.</p></body></html>";
        let (title, blocks) = html_to_blocks(html);
        assert_eq!(title.as_deref(), Some("My Page"));
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[0].kind, BlockKind::Heading);
        assert_eq!(blocks[0].heading_level, Some(1));
        assert_eq!(blocks[1].text, "First para.");
        assert_eq!(blocks[2].heading_level, Some(2));
    }

    #[test]
    fn script_and_style_content_is_never_indexed() {
        let html = "<html><body><script>alert('x')</script><style>.a{color:red}</style><p>Real content.</p></body></html>";
        let (_, blocks) = html_to_blocks(html);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "Real content.");
    }

    #[test]
    fn nested_recognized_tags_are_not_double_counted() {
        // A <p> nested inside a <blockquote> (or a table cell's <p>
        // inside a <table>) must produce exactly one block, not one per
        // recognized tag in the chain — the outermost recognized
        // ancestor wins (right for <table>, whose whole point is one
        // block covering every cell) and its `.text()` already includes
        // the nested tag's text, so the nested one is skipped.
        let html = "<html><body><blockquote><p>Quoted para.</p></blockquote></body></html>";
        let (_, blocks) = html_to_blocks(html);
        assert_eq!(blocks.len(), 1, "only the outermost recognized ancestor should produce a block");
        assert_eq!(blocks[0].text, "Quoted para.");
    }

    #[test]
    fn pre_block_becomes_code() {
        let html = "<html><body><pre>fn main() {}</pre></body></html>";
        let (_, blocks) = html_to_blocks(html);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Code);
    }

    #[test]
    fn page_with_no_title_returns_none() {
        let (title, _) = html_to_blocks("<html><body><p>no title here</p></body></html>");
        assert!(title.is_none());
    }
}
