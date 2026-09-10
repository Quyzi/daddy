//! A deliberately small YAML-front-matter parser.
//!
//! Every mainstream YAML crate that could parse a Markdown/Obsidian
//! front-matter block is either unmaintained (`serde_yaml`, archived by
//! its author in 2024) or has no comparable maintenance track record yet
//! — not something to depend on for a document-shaped feature that only
//! ever needs a handful of flat scalar/list keys (`title`, `tags`,
//! `aliases`, `description`, ...). So, same instinct as `asciidoc.rs`'s
//! hand-rolled parser: support exactly the subset real front matter
//! actually uses, and document the limitation rather than pull in a
//! general-purpose parser for it.
//!
//! Supported:
//! ```yaml
//! title: My Note
//! description: "quoted value"
//! tags: [one, two, three]
//! aliases:
//!   - Old Name
//!   - Other Name
//! ```
//! Not supported: nested maps, multi-line block scalars (`|`/`>`),
//! anchors/aliases, or anything else YAML's full spec allows — a line
//! that doesn't match `key: scalar`, `key: [inline, list]`, or `- item`
//! under a bare `key:` is skipped rather than rejected, so an
//! unrecognized construct just doesn't surface as a field instead of
//! failing the whole ingest.

use serde_json::{Map, Value};

/// Strips a leading `---\n ... \n---\n` block from `text` and parses it.
/// Returns `(frontmatter, rest)`: `frontmatter` is `None` when `text`
/// doesn't open with a front-matter fence at all, and `rest` is always
/// the remainder of the document with the fence removed (or the whole
/// input, unchanged, when there was none).
pub fn extract(text: &str) -> (Option<Value>, &str) {
    let Some(after_open) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return (None, text);
    };
    let Some(close) = find_closing_fence(after_open) else {
        return (None, text);
    };
    let (block, rest) = after_open.split_at(close.0);
    let rest = &rest[close.1..];
    (Some(parse_block(block)), rest)
}

/// Finds `\n---\n` (or `\r\n---\r\n`) marking the end of the front-matter
/// block, returning `(block_end, fence_len)` so the caller can slice the
/// block out and skip past the closing fence in one step.
fn find_closing_fence(s: &str) -> Option<(usize, usize)> {
    let mut search_from = 0;
    loop {
        let rel = s[search_from..].find("\n---")?;
        let pos = search_from + rel;
        let after = &s[pos + 4..];
        if let Some(rest) = after.strip_prefix('\n') {
            return Some((pos + 1, s.len() - rest.len() - pos - 1));
        }
        if let Some(rest) = after.strip_prefix("\r\n") {
            return Some((pos + 1, s.len() - rest.len() - pos - 1));
        }
        if after.is_empty() {
            // Fence is the last line with no trailing newline.
            return Some((pos + 1, s.len() - pos - 1));
        }
        search_from = pos + 4;
    }
}

/// Parses the subset of YAML described in the module docs into a flat
/// JSON object: each key maps to a `Value::String` or a
/// `Value::Array<Value::String>`.
fn parse_block(block: &str) -> Value {
    let mut map = Map::new();
    let lines: Vec<&str> = block.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_end_matches('\r');
        if trimmed.trim().is_empty() || trimmed.trim_start().starts_with('#') {
            i += 1;
            continue;
        }
        let Some(colon) = trimmed.find(':') else {
            i += 1;
            continue;
        };
        let key = trimmed[..colon].trim();
        if key.is_empty() {
            i += 1;
            continue;
        }
        let value_part = trimmed[colon + 1..].trim();

        if value_part.is_empty() {
            // Either a block list on following more-indented `- item`
            // lines, or a genuinely empty scalar.
            let mut items = Vec::new();
            let mut j = i + 1;
            while j < lines.len() {
                let candidate = lines[j].trim_end_matches('\r');
                let stripped = candidate.trim_start();
                if candidate.is_empty() {
                    j += 1;
                    continue;
                }
                if !candidate.starts_with(' ') && !candidate.starts_with('\t') {
                    break;
                }
                let Some(item) = stripped.strip_prefix("- ").or_else(|| stripped.strip_prefix('-')) else {
                    break;
                };
                items.push(Value::String(unquote(item.trim()).to_string()));
                j += 1;
            }
            if items.is_empty() {
                map.insert(key.to_string(), Value::String(String::new()));
            } else {
                map.insert(key.to_string(), Value::Array(items));
                i = j;
                continue;
            }
        } else if let Some(inline) = value_part.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let items = inline
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(unquote(s).to_string()))
                .collect();
            map.insert(key.to_string(), Value::Array(items));
        } else {
            map.insert(key.to_string(), Value::String(unquote(value_part).to_string()));
        }
        i += 1;
    }
    Value::Object(map)
}

/// Strips a single layer of matching `"`/`'` quotes, if present.
fn unquote(s: &str) -> &str {
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Reads `key` back out of a parsed front-matter object as a plain
/// string, if present and scalar. A convenience for the common
/// `title`/`description`-shaped lookups.
pub fn get_str<'a>(fm: &'a Value, key: &str) -> Option<&'a str> {
    fm.get(key)?.as_str()
}

/// Reads `key` back out as a list of strings, accepting either a real
/// YAML list or a single scalar (treated as a one-element list) — front
/// matter in the wild is inconsistent about `tags: solo-tag` vs
/// `tags: [solo-tag]`, and callers (tags, aliases) want both to work.
pub fn get_list(fm: &Value, key: &str) -> Vec<String> {
    match fm.get(key) {
        Some(Value::Array(items)) => {
            items.iter().filter_map(|v| v.as_str()).map(str::to_string).collect()
        }
        Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_frontmatter_returns_the_whole_text_unchanged() {
        let (fm, rest) = extract("# Just a heading\n\nbody text");
        assert!(fm.is_none());
        assert_eq!(rest, "# Just a heading\n\nbody text");
    }

    #[test]
    fn parses_scalars_inline_lists_and_block_lists() {
        let input = "---\ntitle: My Note\ndescription: \"a quoted value\"\ntags: [one, two, three]\naliases:\n  - Old Name\n  - Other Name\n---\n# Body starts here\n";
        let (fm, rest) = extract(input);
        let fm = fm.expect("frontmatter should be detected");
        assert_eq!(get_str(&fm, "title"), Some("My Note"));
        assert_eq!(get_str(&fm, "description"), Some("a quoted value"));
        assert_eq!(get_list(&fm, "tags"), vec!["one", "two", "three"]);
        assert_eq!(get_list(&fm, "aliases"), vec!["Old Name", "Other Name"]);
        assert_eq!(rest, "# Body starts here\n");
    }

    #[test]
    fn unclosed_fence_is_treated_as_no_frontmatter() {
        let input = "---\ntitle: oops\nno closing fence here";
        let (fm, rest) = extract(input);
        assert!(fm.is_none());
        assert_eq!(rest, input);
    }

    #[test]
    fn single_scalar_tag_list_normalizes_to_one_element() {
        let input = "---\ntags: solo-tag\n---\nbody\n";
        let (fm, _) = extract(input);
        let fm = fm.unwrap();
        assert_eq!(get_list(&fm, "tags"), vec!["solo-tag"]);
    }
}
