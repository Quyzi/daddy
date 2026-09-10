//! Chrome detection: running headers/footers (book title, chapter name,
//! page number) repeat at the same position on most pages and carry no
//! indexable content — this module finds them by looking for text that
//! recurs, position-normalized, across a large fraction of a document's
//! pages, and is the only stage in this crate that needs whole-document
//! (not just whole-page) context.

use std::collections::{HashMap, HashSet};

/// Normalizes text for chrome-repetition comparison: lowercased, with
/// runs of digits collapsed to a single `#` so page numbers don't defeat
/// the match (e.g. `"Chapter 3 | 42"` and `"Chapter 3 | 57"` normalize to
/// the same string).
pub fn normalize(text: &str) -> String {
    let mut out = String::new();
    let mut in_digits = false;
    for c in text.trim().chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
            }
            in_digits = true;
        } else {
            in_digits = false;
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

/// Minimum fraction of candidate-bearing pages a normalized string must
/// appear on to be treated as running chrome rather than coincidence.
pub const MIN_REPEAT_FRACTION: f64 = 0.5;

/// Minimum raw occurrence count required in addition to the fraction, so
/// a 3-page document doesn't flag its own front matter as "chrome" just
/// because two pages happen to start with the same word.
pub const MIN_REPEAT_COUNT: usize = 4;

/// Given one normalized candidate string per page (`None` where a page
/// has nothing at that position), returns the set of strings that recur
/// often enough to be running chrome.
pub fn repeated_strings(candidates: &[Option<String>]) -> HashSet<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut total = 0usize;
    for c in candidates.iter().flatten() {
        if c.is_empty() {
            continue;
        }
        *counts.entry(c.clone()).or_insert(0) += 1;
        total += 1;
    }
    if total == 0 {
        return HashSet::new();
    }
    counts
        .into_iter()
        .filter(|(_, n)| *n >= MIN_REPEAT_COUNT && (*n as f64) / (total as f64) >= MIN_REPEAT_FRACTION)
        .map(|(s, _)| s)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_page_numbers() {
        assert_eq!(normalize("Chapter 3 | 42"), normalize("Chapter 3 | 57"));
        assert_eq!(normalize("  Curse of Strahd  "), "curse of strahd");
    }

    #[test]
    fn finds_a_header_repeated_across_most_pages() {
        let candidates: Vec<Option<String>> = (0..10)
            .map(|i| {
                if i == 3 {
                    Some("unique one-off title".to_string())
                } else {
                    Some(normalize(&format!("Curse of Strahd | {i}")))
                }
            })
            .collect();
        let repeated = repeated_strings(&candidates);
        assert!(repeated.contains(&normalize("Curse of Strahd | 99")));
        assert!(!repeated.contains("unique one-off title"));
    }

    #[test]
    fn does_not_flag_a_short_document_on_coincidence() {
        let candidates = vec![Some("intro".to_string()), Some("intro".to_string()), None];
        assert!(repeated_strings(&candidates).is_empty(), "below MIN_REPEAT_COUNT must not trigger");
    }
}
