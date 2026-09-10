//! `.url` (one entry) and `.urls` (a list) files: `raw/urls/` is where a
//! brain records links to fetch and index, rather than files copied in
//! directly.
//!
//! Grammar (identical for both extensions — `.url` is just the
//! one-entry case):
//! ```text
//! /// The 2024 revised Player's Handbook errata thread
//! https://example.com/errata-thread
//!
//! // internal note, never stored or indexed
//! https://example.com/another-source
//! ```
//! - `///` immediately preceding a URL line is a doc comment: it becomes
//!   that entry's title, exactly mirroring how a Rust `///` doc comment
//!   documents the item immediately below it.
//! - `//` is a throwaway comment: stripped entirely, never stored —
//!   mirroring Rust's plain `//`, which compiles to nothing.
//! - A blank line clears any pending `///` title without attaching it to
//!   anything — same rule as Rust, where a doc comment separated from its
//!   item by a blank line doesn't document that item.
//! - Any other non-blank line must parse as an `http://`/`https://` URL;
//!   anything else is a parse error surfaced per-file (not a silent
//!   skip), consistent with how `brain ingest` reports every other kind
//!   of extraction failure.
//!
//! Unlike every other backend, one input file here can yield **many**
//! independent documents — each URL is its own source with its own
//! citation and re-fetch lifecycle, so a `.urls` list of ten links
//! becomes ten `Document` rows, not one.

use crate::html::html_to_blocks;
use crate::structured::{StructuredBlock, StructuredDoc, StructuredExtractor};
use crate::types::Capability;
use brain_core::error::{BrainError, Result};
use std::path::Path;
use std::time::Duration;

const URL_EXTENSIONS: &[&str] = &["url", "urls"];

/// Network timeout for one request.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
/// Response body size cap — protects against a pathological or malicious
/// response exhausting memory (see `ureq::Body::with_config`'s docs).
const MAX_RESPONSE_BYTES: u64 = 25 * 1024 * 1024;
/// Delay between consecutive requests within one `.urls` list — a
/// personal tool fetching a user-curated handful of links doesn't need
/// full crawler-politeness machinery, but a fixed small delay costs
/// nothing and avoids hammering a single host.
const INTER_REQUEST_DELAY: Duration = Duration::from_millis(500);
/// Identifies this tool to servers, distinctly from a browser — so a
/// site operator looking at their logs can tell what's requesting.
const USER_AGENT: &str = "brain/0.1 (personal knowledge-base indexer; https://github.com/Quyzi/daddy)";

/// One parsed entry from a `.url`/`.urls` file, before fetching.
#[derive(Debug, Clone, PartialEq)]
pub struct UrlEntry {
    /// The URL to fetch.
    pub url: String,
    /// The preceding `///` doc comment, if any — becomes the resulting
    /// document's title when present.
    pub title: Option<String>,
}

/// Parses the grammar described in the module docs.
pub fn parse_url_file(text: &str) -> Result<Vec<UrlEntry>> {
    let mut entries = Vec::new();
    let mut pending_title: Option<String> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            // A doc comment not immediately followed by a URL doesn't
            // attach to anything — same rule as Rust's `///`.
            pending_title = None;
            continue;
        }
        if let Some(doc) = line.strip_prefix("///") {
            pending_title = Some(doc.trim().to_string());
            continue;
        }
        if line.starts_with("//") {
            continue;
        }
        if line.starts_with("http://") || line.starts_with("https://") {
            entries.push(UrlEntry { url: line.to_string(), title: pending_title.take() });
            continue;
        }
        return Err(BrainError::InvalidData(format!(
            "not a URL, a `//`/`///` comment, or blank: {line:?}"
        )));
    }
    Ok(entries)
}

/// Fetches `url` and parses it into structural blocks via
/// [`html_to_blocks`]. Returns `(effective_title, blocks)` — the page's
/// `<title>`, when the response didn't already have one supplied by a
/// `///` doc comment.
fn fetch_and_parse(url: &str) -> Result<(Option<String>, Vec<StructuredBlock>)> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(FETCH_TIMEOUT))
        .user_agent(USER_AGENT)
        .build();
    let agent: ureq::Agent = config.into();

    let mut response = agent
        .get(url)
        .call()
        .map_err(|e| BrainError::Extraction(format!("fetching {url}: {e}")))?;
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|e| BrainError::Extraction(format!("reading response body from {url}: {e}")))?;

    Ok(html_to_blocks(&body))
}

/// Extracts `.url`/`.urls` files: parses the entry list, fetches and
/// structurally parses each one, and returns one [`StructuredDoc`] per
/// URL — see this module's docs for why that's a one-to-many mapping,
/// unlike every other [`StructuredExtractor`].
#[derive(Default)]
pub struct UrlExtractor;

impl UrlExtractor {
    /// Creates a new URL extractor. Stateless — this never fails.
    pub fn new() -> Self {
        Self
    }
}

impl StructuredExtractor for UrlExtractor {
    fn name(&self) -> &'static str {
        "url"
    }

    fn probe(&self, path: &Path) -> Result<Capability> {
        let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
        let supported = matches!(ext.as_deref(), Some(e) if URL_EXTENSIONS.contains(&e));
        Ok(Capability { supported, page_count: None, note: None })
    }

    fn extract_structured(&self, path: &Path) -> Result<Vec<StructuredDoc>> {
        let text = std::fs::read_to_string(path)?;
        let entries = parse_url_file(&text)
            .map_err(|e| BrainError::Extraction(format!("{}: {e}", path.display())))?;

        let mut docs = Vec::with_capacity(entries.len());
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                std::thread::sleep(INTER_REQUEST_DELAY);
            }
            let (page_title, blocks) = fetch_and_parse(&entry.url)?;
            let title = entry.title.clone().or(page_title).unwrap_or_else(|| entry.url.clone());
            let frontmatter = entry
                .title
                .as_ref()
                .map(|t| serde_json::json!({ "title": t }).to_string());
            docs.push(StructuredDoc { title, source_path: entry.url.clone(), frontmatter, blocks });
        }
        Ok(docs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_doc_comments_plain_comments_and_urls() {
        let text = "\
/// The 2024 revised Player's Handbook errata thread
https://example.com/errata-thread

// internal note, never stored or indexed
https://example.com/another-source
";
        let entries = parse_url_file(text).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].url, "https://example.com/errata-thread");
        assert_eq!(entries[0].title.as_deref(), Some("The 2024 revised Player's Handbook errata thread"));
        assert_eq!(entries[1].url, "https://example.com/another-source");
        assert_eq!(entries[1].title, None);
    }

    #[test]
    fn a_doc_comment_separated_by_a_blank_line_does_not_attach() {
        let text = "/// orphaned title\n\nhttps://example.com/x\n";
        let entries = parse_url_file(text).unwrap();
        assert_eq!(entries[0].title, None);
    }

    #[test]
    fn a_single_url_file_is_the_one_entry_case_of_the_same_grammar() {
        let entries = parse_url_file("https://example.com/solo\n").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url, "https://example.com/solo");
    }

    #[test]
    fn a_malformed_line_is_a_parse_error_not_a_silent_skip() {
        let err = parse_url_file("not a url or a comment\n").unwrap_err();
        assert!(err.to_string().contains("not a URL"));
    }

    #[test]
    fn probe_accepts_url_and_urls_extensions_only() {
        let ex = UrlExtractor::new();
        assert!(ex.probe(Path::new("reading-list.urls")).unwrap().supported);
        assert!(ex.probe(Path::new("single.url")).unwrap().supported);
        assert!(!ex.probe(Path::new("notes.md")).unwrap().supported);
    }
}
