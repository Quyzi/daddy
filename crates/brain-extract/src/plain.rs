//! Extraction backend for already-textual sources (`.txt`, `.md`, `.html`,
//! `.json`, `.csv`, ...) that have no page geometry of their own. Each
//! source becomes a single synthetic page whose "geometry" just encodes
//! line/word order, so `brain-layout` can treat it identically to a PDF
//! page (trivially — one column, headings still detected from blank-line
//! and Markdown-heading conventions).

use crate::types::{Capability, Extractor, PageRange};
use brain_core::error::Result;
use brain_core::{BBox, RawPage, Word};
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

const PLAIN_EXTENSIONS: &[&str] = &["txt", "md", "markdown", "log", "adoc", "rst", "csv"];
const MARKUP_EXTENSIONS: &[&str] = &["html", "htm", "xml"];

/// Reads plain-text-ish files directly, synthesizing word positions from
/// line/column order so downstream layout code needs no special case for
/// non-paginated sources.
#[derive(Default)]
pub struct PlainExtractor;

impl PlainExtractor {
    /// Creates a new plain-text extractor. Stateless — this never fails.
    pub fn new() -> Self {
        Self
    }
}

fn tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)<[^>]+>").unwrap())
}

/// Very small HTML-to-text step: strips tags and collapses a handful of
/// common entities. This is not a real HTML parser — it exists so a
/// scraped article's prose is indexable, not to preserve markup fidelity.
fn strip_markup(input: &str) -> String {
    let no_tags = tag_regex().replace_all(input, "\n");
    no_tags
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// Turns already-linear text into one [`RawPage`] whose words carry
/// synthetic, order-preserving bounding boxes: each line gets its own
/// `y`, each word within it a monotonically increasing `x`.
fn text_to_page(text: &str) -> RawPage {
    let mut words = Vec::new();
    let mut max_x = 0.0f64;
    let mut y = 0.0f64;
    for line in text.lines() {
        let mut x = 0.0f64;
        let mut any = false;
        for tok in line.split_whitespace() {
            let width = tok.chars().count().max(1) as f64;
            words.push(Word {
                text: tok.to_string(),
                bbox: BBox { x0: x, y0: y, x1: x + width, y1: y + 1.0 },
                confidence: None,
            });
            x += width + 1.0;
            any = true;
        }
        if any {
            max_x = max_x.max(x);
            y += 1.0;
        } else {
            // Blank line: still advance y so paragraph breaks survive as a
            // detectable gap, but emit no words for it.
            y += 1.0;
        }
    }
    RawPage {
        page_no: 1,
        width: max_x.max(1.0),
        height: y.max(1.0),
        words,
        ocr_confidence: None,
    }
}

impl Extractor for PlainExtractor {
    fn name(&self) -> &'static str {
        "plain"
    }

    fn probe(&self, path: &Path) -> Result<Capability> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let supported = matches!(ext.as_deref(), Some(e) if PLAIN_EXTENSIONS.contains(&e) || MARKUP_EXTENSIONS.contains(&e));
        Ok(Capability { supported, page_count: Some(1), note: None })
    }

    fn extract(&self, path: &Path, _pages: PageRange) -> Result<Vec<RawPage>> {
        let raw = std::fs::read_to_string(path)?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        let text = if MARKUP_EXTENSIONS.contains(&ext.as_str()) {
            strip_markup(&raw)
        } else {
            raw
        };
        Ok(vec![text_to_page(&text)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesizes_ordered_words_from_lines() {
        let page = text_to_page("hello world\nsecond line");
        assert_eq!(page.words.len(), 4);
        assert_eq!(page.words[0].text, "hello");
        assert_eq!(page.words[1].text, "world");
        assert!(page.words[0].bbox.x0 < page.words[1].bbox.x0);
        assert!(page.words[0].bbox.y0 < page.words[2].bbox.y0);
    }

    #[test]
    fn strips_html_tags_and_common_entities() {
        let out = strip_markup("<p>Fire &amp; Ice</p>");
        assert!(out.contains("Fire & Ice"));
        assert!(!out.contains('<'));
    }

    #[test]
    fn probe_accepts_known_text_extensions_only() {
        let ex = PlainExtractor::new();
        assert!(ex.probe(Path::new("notes.md")).unwrap().supported);
        assert!(!ex.probe(Path::new("book.pdf")).unwrap().supported);
    }
}
