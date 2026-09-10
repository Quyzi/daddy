//! Obsidian-style `[[wikilink]]`/`#tag` handling, applied uniformly to
//! every Markdown file rather than only inside a detected `.obsidian/`
//! vault — an Obsidian vault is just a folder of Markdown files with
//! editor config alongside it, and treating these as ordinary Markdown
//! extensions (also used by Logseq, Foam, and others) means no fragile
//! vault-detection heuristic is needed anywhere in the pipeline.
//!
//! Resolution against the rest of the graph — matching a wikilink's
//! target to another ingested document, upserting a tag as an entity,
//! recording a frontmatter alias — happens in
//! [`crate::orchestrate::index_all`], once every document is known (the
//! same phase as the gazetteer's whole-corpus mention scan). This module
//! only extracts the raw syntax from block/chunk text and provides the
//! small shared helpers (name normalization, frontmatter list reading)
//! that both the index-time resolver and `brain-wiki`'s lint check need.

use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

/// One `[[wikilink]]` found in a chunk's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    /// The link target as written — a note title/slug, not yet resolved
    /// against any document.
    pub target: String,
    /// The `|Alias` display text, if given. Not currently used for
    /// resolution (targets are matched, not aliases), but kept for a
    /// future "alias this document" enhancement and for fidelity when
    /// rendering a link back out.
    pub alias: Option<String>,
}

fn wikilink_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // `!?` optionally matches Obsidian's embed prefix (`![[Note]]`) —
    // embeds are still a reference to another note for graph purposes,
    // so they're treated identically to a plain link.
    RE.get_or_init(|| Regex::new(r"!?\[\[([^\]|]+)(?:\|([^\]]+))?\]\]").unwrap())
}

fn tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // A tag must start at the beginning of the text or after whitespace
    // — otherwise `https://example.com/page#section` would false-positive
    // on `#section`. `(?:^|\s)` is non-capturing so group 1 is just the
    // tag word, with the delimiter char (if any) excluded from the match
    // (a plain `#` without a following word character is punctuation,
    // not a tag, so is deliberately not matched).
    RE.get_or_init(|| Regex::new(r"(?:^|\s)#([\p{L}][\w/-]*)").unwrap())
}

/// Extracts every `[[wikilink]]`/`![[embed]]` from `text`, in order.
pub fn extract_wikilinks(text: &str) -> Vec<WikiLink> {
    wikilink_regex()
        .captures_iter(text)
        .map(|c| WikiLink {
            target: c.get(1).unwrap().as_str().trim().to_string(),
            alias: c.get(2).map(|m| m.as_str().trim().to_string()),
        })
        .collect()
}

/// Extracts every `#tag` from `text`, in order (duplicates included —
/// callers that only care about the distinct set should dedupe).
pub fn extract_tags(text: &str) -> Vec<String> {
    tag_regex().captures_iter(text).map(|c| c.get(1).unwrap().as_str().to_string()).collect()
}

/// Normalizes a name for alias lookup: lowercased, internal whitespace
/// collapsed to single spaces — the convention
/// `brain_store::Store::add_alias`'s docs describe.
pub fn normalize_alias(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Reads a front-matter key back out as a list of strings, accepting
/// either a real JSON array or a single string (treated as a one-element
/// list) — mirrors `brain_extract::frontmatter::get_list`'s leniency,
/// duplicated here in miniature rather than pulling in a dependency on
/// `brain-extract` for five lines of logic this crate has no other need
/// for.
pub fn frontmatter_list(fm: &Value, key: &str) -> Vec<String> {
    match fm.get(key) {
        Some(Value::Array(items)) => items.iter().filter_map(|v| v.as_str()).map(str::to_string).collect(),
        Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_aliased_and_embed_links() {
        let text = "See [[Other Note]], [[Real Name|Display Text]], and ![[Embedded Note]] for more.";
        let links = extract_wikilinks(text);
        assert_eq!(links.len(), 3);
        assert_eq!(links[0], WikiLink { target: "Other Note".to_string(), alias: None });
        assert_eq!(links[1], WikiLink { target: "Real Name".to_string(), alias: Some("Display Text".to_string()) });
        assert_eq!(links[2], WikiLink { target: "Embedded Note".to_string(), alias: None });
    }

    #[test]
    fn extracts_tags_but_not_url_fragments() {
        let text = "Tagged #important-tag here, but not https://example.com/page#section.";
        let tags = extract_tags(text);
        assert_eq!(tags, vec!["important-tag"]);
    }

    #[test]
    fn tag_at_start_of_text_is_still_matched() {
        assert_eq!(extract_tags("#leading tag"), vec!["leading"]);
    }

    #[test]
    fn normalize_alias_lowercases_and_collapses_whitespace() {
        assert_eq!(normalize_alias("  Old   Name  "), "old name");
    }

    #[test]
    fn frontmatter_list_accepts_array_or_bare_scalar() {
        let fm: Value = serde_json::json!({ "tags": ["a", "b"], "aliases": "solo" });
        assert_eq!(frontmatter_list(&fm, "tags"), vec!["a", "b"]);
        assert_eq!(frontmatter_list(&fm, "aliases"), vec!["solo"]);
        assert!(frontmatter_list(&fm, "missing").is_empty());
    }
}
