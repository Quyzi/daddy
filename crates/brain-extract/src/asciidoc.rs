//! Structural AsciiDoc extraction.
//!
//! No pure-Rust AsciiDoc parser has a maintenance track record comparable
//! to `pulldown-cmark`'s yet, so this is a small, hand-rolled, line-based
//! parser covering the subset of the spec actually used in personal
//! notes and rulebook excerpts — the same "pragmatic subset, not full
//! spec" instinct behind `brain-layout`'s heading ensemble (a tuned
//! heuristic, not a general solution) and `frontmatter.rs`'s minimal
//! YAML. Supported:
//!
//! - `= Title` / `== H2` / ... — heading level is the run of leading `=`.
//! - `:attr: value` — document attribute entries, collected as metadata
//!   (this project's stand-in for Markdown front matter).
//! - `//` line comment — stripped, never stored.
//! - `////` ... `////` block comment — stripped entirely.
//! - `[source,lang]` (optional) followed by a `----`...`----` delimited
//!   block — [`brain_core::BlockKind::Code`], stored with the same
//!   `` ```lang `` prefix convention `markdown.rs` uses, so downstream
//!   code never needs a per-format code-block representation.
//! - `====`/`****`/`____` delimited blocks (example/sidebar/quote) —
//!   kept as [`brain_core::BlockKind::Body`]; the semantic distinction
//!   between them isn't preserved (no indexing value in a `BlockKind`
//!   variant per delimiter type — the text is still fully searchable).
//! - Everything else — paragraph [`brain_core::BlockKind::Body`], blank
//!   lines end a paragraph.
//!
//! Anything outside this subset (nested includes, cross-reference
//! resolution, tables, callouts, ...) passes through as plain paragraph
//! text rather than causing a parse failure — a personal notes file that
//! uses a fancier construct still gets indexed, just without that
//! construct's special structure recognized.

use crate::structured::{synthetic_bbox, StructuredBlock, StructuredDoc, StructuredExtractor};
use crate::types::Capability;
use brain_core::error::Result;
use brain_core::BlockKind;
use serde_json::{Map, Value};
use std::path::Path;

/// Extracts `.adoc` files.
#[derive(Default)]
pub struct AsciidocExtractor;

impl AsciidocExtractor {
    /// Creates a new AsciiDoc extractor. Stateless — this never fails.
    pub fn new() -> Self {
        Self
    }
}

impl StructuredExtractor for AsciidocExtractor {
    fn name(&self) -> &'static str {
        "asciidoc"
    }

    fn probe(&self, path: &Path) -> Result<Capability> {
        let supported = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("adoc"));
        Ok(Capability { supported, page_count: Some(1), note: None })
    }

    fn extract_structured(&self, path: &Path) -> Result<Vec<StructuredDoc>> {
        let raw = std::fs::read_to_string(path)?;
        let (attrs, blocks) = parse_asciidoc(&raw);

        let filename_title =
            path.file_stem().and_then(|s| s.to_str()).unwrap_or("untitled").to_string();
        // A `= Title` heading at document level 1 doubles as the
        // AsciiDoc document title by convention; prefer it, then a
        // `:title:` attribute, then the filename.
        let title = blocks
            .iter()
            .find(|b| b.kind == BlockKind::Heading && b.heading_level == Some(1))
            .map(|b| b.text.clone())
            .or_else(|| attrs.get("title").and_then(|v| v.as_str()).map(str::to_string))
            .unwrap_or(filename_title);

        let frontmatter =
            if attrs.is_empty() { None } else { Some(Value::Object(attrs).to_string()) };

        Ok(vec![StructuredDoc { title, source_path: path.display().to_string(), frontmatter, blocks }])
    }
}

/// Parses AsciiDoc source into `(document attributes, blocks)`.
fn parse_asciidoc(body: &str) -> (Map<String, Value>, Vec<StructuredBlock>) {
    let mut attrs = Map::new();
    let mut blocks = Vec::new();
    let mut ord: u32 = 0;
    let mut para: Vec<&str> = Vec::new();
    let mut pending_lang: Option<String> = None;

    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;

    macro_rules! flush_para {
        () => {
            if !para.is_empty() {
                push_block(&mut blocks, &mut ord, BlockKind::Body, None, para.join("\n"));
                para.clear();
            }
        };
    }

    while i < lines.len() {
        let line = lines[i].trim_end_matches('\r');
        let stripped = line.trim();

        if stripped.is_empty() {
            flush_para!();
            i += 1;
            continue;
        }
        if stripped == "////" {
            flush_para!();
            i = skip_delimited(&lines, i + 1, "////");
            continue;
        }
        if stripped.starts_with("//") {
            i += 1;
            continue;
        }
        if let Some((key, value)) = parse_attribute_entry(stripped) {
            flush_para!();
            attrs.insert(key, Value::String(value));
            i += 1;
            continue;
        }
        if let Some(lang) = parse_source_attribute(stripped) {
            flush_para!();
            pending_lang = lang;
            i += 1;
            continue;
        }
        if let Some(level) = heading_level(stripped) {
            flush_para!();
            let title = stripped[level as usize..].trim().to_string();
            push_block(&mut blocks, &mut ord, BlockKind::Heading, Some(level), title);
            i += 1;
            continue;
        }
        if stripped == "----" {
            flush_para!();
            let lang = pending_lang.take().unwrap_or_default();
            let (content_end, next) = find_delimited_end(&lines, i + 1, "----");
            let code = lines[i + 1..content_end].join("\n");
            push_block(&mut blocks, &mut ord, BlockKind::Code, None, format!("```{lang}\n{code}```"));
            i = next;
            continue;
        }
        if matches!(stripped, "====" | "****" | "____") {
            flush_para!();
            let delim = stripped.to_string();
            let (content_end, next) = find_delimited_end(&lines, i + 1, &delim);
            let text = lines[i + 1..content_end].join("\n");
            if !text.trim().is_empty() {
                push_block(&mut blocks, &mut ord, BlockKind::Body, None, text);
            }
            i = next;
            continue;
        }

        para.push(line);
        i += 1;
    }
    flush_para!();
    (attrs, blocks)
}

fn push_block(blocks: &mut Vec<StructuredBlock>, ord: &mut u32, kind: BlockKind, heading_level: Option<u8>, text: String) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    blocks.push(StructuredBlock { col: 0, ord: *ord, bbox: synthetic_bbox(*ord), kind, text, heading_level });
    *ord += 1;
}

/// Skips lines until (and past) a line exactly equal to `delim`, used for
/// block comments whose content is discarded entirely. Returns the index
/// just past the closing delimiter (or `lines.len()` if it's never
/// found — an unterminated block comment silently consumes the rest of
/// the file, which is at least safe, if not spec-perfect).
fn skip_delimited(lines: &[&str], start: usize, delim: &str) -> usize {
    let mut i = start;
    while i < lines.len() && lines[i].trim_end_matches('\r').trim() != delim {
        i += 1;
    }
    (i + 1).min(lines.len() + 1)
}

/// Finds a closing delimiter line, returning `(content_end, next_index)`
/// — `content_end` is the index of the closing delimiter itself (so
/// `lines[start..content_end]` is exactly the block's content), `next`
/// is the index to resume parsing from.
fn find_delimited_end(lines: &[&str], start: usize, delim: &str) -> (usize, usize) {
    let mut i = start;
    while i < lines.len() && lines[i].trim_end_matches('\r').trim() != delim {
        i += 1;
    }
    (i, (i + 1).min(lines.len()))
}

/// `= Title` / `== H2` / ... -> `Some(level)`. Requires a space after the
/// run of `=` (AsciiDoc's own rule, and it keeps `===` used as arbitrary
/// emphasis-free punctuation from misparsing as a heading).
fn heading_level(line: &str) -> Option<u8> {
    let eq_count = line.chars().take_while(|&c| c == '=').count();
    if eq_count == 0 || eq_count > 6 {
        return None;
    }
    let rest = &line[eq_count..];
    if rest.starts_with(' ') && !rest.trim().is_empty() {
        Some(eq_count as u8)
    } else {
        None
    }
}

/// `:name: value` -> `Some((name, value))`. Rejects AsciiDoc's
/// description-list syntax (`term:: definition`) by requiring the name
/// contain no whitespace and the line have a *second* colon.
fn parse_attribute_entry(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix(':')?;
    let colon = rest.find(':')?;
    let name = &rest[..colon];
    if name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    let value = rest[colon + 1..].trim().to_string();
    Some((name.to_string(), value))
}

/// `[source]` or `[source,lang]` -> `Some(lang)` (empty string if no
/// language given). Any other bracketed attribute line (`[NOTE]`,
/// `[quote, Author]`, ...) returns `None` and is treated as an ordinary
/// paragraph line — attribute lists beyond `[source]` aren't part of the
/// subset this parser targets.
fn parse_source_attribute(line: &str) -> Option<Option<String>> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    let mut parts = inner.split(',');
    if parts.next()?.trim() != "source" {
        return None;
    }
    let lang = parts.next().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Some(lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_carry_real_depth_and_document_title_wins() {
        let (_, blocks) = parse_asciidoc("= Document Title\n\n== Section One\n\nSome body text.\n\n=== Sub Section\n");
        let headings: Vec<(&str, Option<u8>)> =
            blocks.iter().filter(|b| b.kind == BlockKind::Heading).map(|b| (b.text.as_str(), b.heading_level)).collect();
        assert_eq!(headings, vec![("Document Title", Some(1)), ("Section One", Some(2)), ("Sub Section", Some(3))]);
    }

    #[test]
    fn attribute_entries_become_metadata_not_body_text() {
        let (attrs, blocks) = parse_asciidoc(":author: Jane Doe\n:revdate: 2024-01-01\n\n= Title\n\nbody\n");
        assert_eq!(attrs.get("author").and_then(|v| v.as_str()), Some("Jane Doe"));
        assert!(!blocks.iter().any(|b| b.text.contains("Jane Doe")));
    }

    #[test]
    fn line_and_block_comments_are_stripped_entirely() {
        let (_, blocks) = parse_asciidoc(
            "// a throwaway note\n= Title\n\n////\nthis whole block\nis a comment\n////\n\nvisible body\n",
        );
        let all_text: String = blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join(" ");
        assert!(!all_text.contains("throwaway"));
        assert!(!all_text.contains("this whole block"));
        assert!(all_text.contains("visible body"));
    }

    #[test]
    fn source_block_becomes_code_with_language_prefix() {
        let (_, blocks) = parse_asciidoc("[source,rust]\n----\nfn main() {}\n----\n");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Code);
        assert!(blocks[0].text.starts_with("```rust\n"));
        assert!(blocks[0].text.contains("fn main()"));
    }

    #[test]
    fn example_sidebar_and_quote_blocks_become_body() {
        let (_, blocks) = parse_asciidoc("====\nan example\n====\n\n****\na sidebar\n****\n\n____\na quote\n____\n");
        assert_eq!(blocks.len(), 3);
        assert!(blocks.iter().all(|b| b.kind == BlockKind::Body));
        assert_eq!(blocks[0].text, "an example");
        assert_eq!(blocks[2].text, "a quote");
    }

    #[test]
    fn plain_paragraphs_join_consecutive_lines() {
        let (_, blocks) = parse_asciidoc("line one\nline two\n\nnew paragraph\n");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "line one\nline two");
        assert_eq!(blocks[1].text, "new paragraph");
    }
}
