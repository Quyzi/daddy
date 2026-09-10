//! Structural Markdown extraction via `pulldown-cmark` — a real parse of
//! the document's own declared structure (headings with real depth,
//! fenced code, tables, front matter) instead of routing through
//! `PlainExtractor` and re-guessing it with `brain-layout`'s PDF-oriented
//! heuristics (see `structured`'s module docs for why that distinction
//! matters).
//!
//! `[[wikilink]]`/`#tag` are not CommonMark syntax, so they are *not*
//! specially parsed here — they simply pass through as ordinary text
//! inside whatever paragraph/heading contains them (pulldown-cmark has
//! no opinion about double brackets or hash-prefixed words), which is
//! exactly what lets `brain_index::wikilink` find them later with a
//! plain regex pass over stored block text, unconcerned with how that
//! text reached storage.

use crate::frontmatter;
use crate::structured::{synthetic_bbox, StructuredBlock, StructuredDoc, StructuredExtractor};
use crate::types::Capability;
use brain_core::error::Result;
use brain_core::BlockKind;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::path::Path;

const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown"];

/// Extracts Markdown files (including Obsidian notes — see
/// `brain_index::wikilink` for the wikilink/tag handling layered on top,
/// applied uniformly to every Markdown file rather than only inside a
/// detected `.obsidian/` vault).
#[derive(Default)]
pub struct MarkdownExtractor;

impl MarkdownExtractor {
    /// Creates a new Markdown extractor. Stateless — this never fails.
    pub fn new() -> Self {
        Self
    }
}

impl StructuredExtractor for MarkdownExtractor {
    fn name(&self) -> &'static str {
        "markdown"
    }

    fn probe(&self, path: &Path) -> Result<Capability> {
        let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
        let supported = matches!(ext.as_deref(), Some(e) if MARKDOWN_EXTENSIONS.contains(&e));
        Ok(Capability { supported, page_count: Some(1), note: None })
    }

    fn extract_structured(&self, path: &Path) -> Result<Vec<StructuredDoc>> {
        let raw = std::fs::read_to_string(path)?;
        let (fm, body) = frontmatter::extract(&raw);

        let filename_title =
            path.file_stem().and_then(|s| s.to_str()).unwrap_or("untitled").to_string();
        let title = fm
            .as_ref()
            .and_then(|v| frontmatter::get_str(v, "title"))
            .map(str::to_string)
            .unwrap_or(filename_title);

        let blocks = parse_markdown(body);
        let doc = StructuredDoc {
            title,
            source_path: path.display().to_string(),
            frontmatter: fm.map(|v| v.to_string()),
            blocks,
        };
        Ok(vec![doc])
    }
}

/// One open block-level context, holding the text accumulated inside it
/// so far. Kept on a stack so arbitrarily nested containers (a list item
/// containing a paragraph, a block quote containing a list, ...) each
/// flush their own text independently — see the module-level design note
/// in this file's git history / the implementation plan for why a stack
/// beats a single flat buffer here.
struct OpenBlock {
    kind: BlockKind,
    heading_level: Option<u8>,
    lang: Option<String>,
    text: String,
}

/// Table-specific accumulator: tables have a fixed, shallow grammar
/// (Table > TableHead|TableRow > TableCell), simple enough to track with
/// plain row/cell buffers rather than reusing the generic stack.
#[derive(Default)]
struct TableState {
    rows: Vec<Vec<String>>,
    current_row: Vec<String>,
    current_cell: String,
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Walks a CommonMark event stream, producing one [`StructuredBlock`]
/// per top-level paragraph/heading/code-block/list-item/block-quote/
/// table, in document order.
fn parse_markdown(body: &str) -> Vec<StructuredBlock> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let parser = Parser::new_ext(body, options);

    let mut blocks = Vec::new();
    let mut ord: u32 = 0;
    let mut stack: Vec<OpenBlock> = Vec::new();
    let mut table: Option<TableState> = None;

    let push_text = |stack: &mut Vec<OpenBlock>, s: &str| {
        if let Some(top) = stack.last_mut() {
            top.text.push_str(s);
        }
    };

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => stack.push(OpenBlock {
                    kind: BlockKind::Body,
                    heading_level: None,
                    lang: None,
                    text: String::new(),
                }),
                Tag::Heading { level, .. } => stack.push(OpenBlock {
                    kind: BlockKind::Heading,
                    heading_level: Some(heading_level_to_u8(level)),
                    lang: None,
                    text: String::new(),
                }),
                Tag::BlockQuote(_) | Tag::Item | Tag::FootnoteDefinition(_) => stack.push(OpenBlock {
                    kind: BlockKind::Body,
                    heading_level: None,
                    lang: None,
                    text: String::new(),
                }),
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(lang) if !lang.is_empty() => Some(lang.to_string()),
                        _ => None,
                    };
                    stack.push(OpenBlock { kind: BlockKind::Code, heading_level: None, lang, text: String::new() });
                }
                Tag::Table(_) => table = Some(TableState::default()),
                Tag::TableRow | Tag::TableHead => { /* row boundary handled at End */ }
                Tag::TableCell => { /* cell text accumulates into table.current_cell */ }
                _ => {} // Emphasis/Strong/Link/Image/List/... carry no text of their own
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Paragraph | TagEnd::BlockQuote(_) | TagEnd::Item | TagEnd::FootnoteDefinition => {
                    flush(&mut stack, &mut blocks, &mut ord);
                }
                TagEnd::Heading(_) => flush(&mut stack, &mut blocks, &mut ord),
                TagEnd::CodeBlock => flush_code(&mut stack, &mut blocks, &mut ord),
                TagEnd::TableCell => {
                    if let Some(t) = table.as_mut() {
                        let cell = std::mem::take(&mut t.current_cell);
                        t.current_row.push(cell.trim().to_string());
                    }
                }
                TagEnd::TableRow | TagEnd::TableHead => {
                    if let Some(t) = table.as_mut() {
                        let row = std::mem::take(&mut t.current_row);
                        t.rows.push(row);
                    }
                }
                TagEnd::Table => {
                    if let Some(t) = table.take() {
                        flush_table(t, &mut blocks, &mut ord);
                    }
                }
                _ => {}
            },
            Event::Text(text) | Event::Code(text) | Event::InlineMath(text) | Event::DisplayMath(text) => {
                if let Some(t) = table.as_mut() {
                    t.current_cell.push_str(&text);
                } else {
                    push_text(&mut stack, &text);
                }
            }
            Event::Html(text) | Event::InlineHtml(text) => push_text(&mut stack, &text),
            Event::FootnoteReference(label) => push_text(&mut stack, &format!("[^{label}]")),
            Event::SoftBreak => {
                if let Some(t) = table.as_mut() {
                    t.current_cell.push(' ');
                } else {
                    push_text(&mut stack, " ");
                }
            }
            Event::HardBreak => push_text(&mut stack, "\n"),
            Event::Rule => {}
            Event::TaskListMarker(checked) => {
                push_text(&mut stack, if checked { "[x] " } else { "[ ] " });
            }
        }
    }
    // A malformed/truncated document could leave open contexts; flush
    // whatever's left rather than silently dropping trailing text.
    while !stack.is_empty() {
        flush(&mut stack, &mut blocks, &mut ord);
    }
    blocks
}

fn flush(stack: &mut Vec<OpenBlock>, blocks: &mut Vec<StructuredBlock>, ord: &mut u32) {
    let Some(open) = stack.pop() else { return };
    let text = open.text.trim();
    if text.is_empty() {
        return;
    }
    blocks.push(StructuredBlock {
        col: 0,
        ord: *ord,
        bbox: synthetic_bbox(*ord),
        kind: open.kind,
        text: text.to_string(),
        heading_level: open.heading_level,
    });
    *ord += 1;
}

fn flush_code(stack: &mut Vec<OpenBlock>, blocks: &mut Vec<StructuredBlock>, ord: &mut u32) {
    let Some(open) = stack.pop() else { return };
    let lang = open.lang.unwrap_or_default();
    let text = format!("```{lang}\n{}```", open.text);
    blocks.push(StructuredBlock {
        col: 0,
        ord: *ord,
        bbox: synthetic_bbox(*ord),
        kind: BlockKind::Code,
        text,
        heading_level: None,
    });
    *ord += 1;
}

fn flush_table(table: TableState, blocks: &mut Vec<StructuredBlock>, ord: &mut u32) {
    if table.rows.is_empty() {
        return;
    }
    let text = table.rows.iter().map(|row| row.join(" | ")).collect::<Vec<_>>().join("\n");
    blocks.push(StructuredBlock {
        col: 0,
        ord: *ord,
        bbox: synthetic_bbox(*ord),
        kind: BlockKind::Table,
        text,
        heading_level: None,
    });
    *ord += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heading_texts(blocks: &[StructuredBlock]) -> Vec<(&str, Option<u8>)> {
        blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Heading)
            .map(|b| (b.text.as_str(), b.heading_level))
            .collect()
    }

    #[test]
    fn headings_carry_real_depth() {
        let blocks = parse_markdown("# Title\n\n## Sub\n\nbody text\n\n### Sub sub\n");
        assert_eq!(heading_texts(&blocks), vec![("Title", Some(1)), ("Sub", Some(2)), ("Sub sub", Some(3))]);
    }

    #[test]
    fn fenced_code_becomes_a_code_block_with_language_prefix() {
        let blocks = parse_markdown("```rust\nfn main() {}\n```\n");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Code);
        assert!(blocks[0].text.starts_with("```rust\n"));
        assert!(blocks[0].text.contains("fn main()"));
    }

    #[test]
    fn wikilinks_and_tags_pass_through_as_plain_text() {
        let blocks = parse_markdown("See [[Other Note]] and #important-tag for context.\n");
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].text.contains("[[Other Note]]"));
        assert!(blocks[0].text.contains("#important-tag"));
    }

    #[test]
    fn table_becomes_one_pipe_joined_block() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n";
        let blocks = parse_markdown(md);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Table);
        assert!(blocks[0].text.contains("A | B"));
        assert!(blocks[0].text.contains("1 | 2"));
    }

    #[test]
    fn list_items_become_body_blocks() {
        let blocks = parse_markdown("- one\n- two\n- three\n");
        assert_eq!(blocks.len(), 3);
        assert!(blocks.iter().all(|b| b.kind == BlockKind::Body));
        assert_eq!(blocks[0].text, "one");
        assert_eq!(blocks[2].text, "three");
    }

    #[test]
    fn frontmatter_title_overrides_filename() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("original-name.md");
        std::fs::write(&path, "---\ntitle: Real Title\ntags: [a, b]\n---\n# Heading\n\nbody\n").unwrap();
        let docs = MarkdownExtractor::new().extract_structured(&path).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].title, "Real Title");
        let fm: serde_json::Value = serde_json::from_str(docs[0].frontmatter.as_ref().unwrap()).unwrap();
        assert_eq!(frontmatter::get_list(&fm, "tags"), vec!["a", "b"]);
    }

    #[test]
    fn no_frontmatter_falls_back_to_filename_stem() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("my-note.md");
        std::fs::write(&path, "# Heading\n\nbody\n").unwrap();
        let docs = MarkdownExtractor::new().extract_structured(&path).unwrap();
        assert_eq!(docs[0].title, "my-note");
        assert!(docs[0].frontmatter.is_none());
    }
}
