//! Heading detection.
//!
//! Font size alone does not separate headings from body text in this
//! corpus — the implementation plan's probing of the Player's Handbook
//! found a heading line at 7.95pt sitting next to 7.01pt body text, a
//! difference too small to threshold reliably across scanned books with
//! wildly varying OCR-reported sizes. Instead this uses a small weighted
//! ensemble of layout-independent signals — being alone on its own line,
//! being short, lacking sentence-ending punctuation, and being title- or
//! all-caps — which together are far more robust than any single signal.

use crate::blocks::RawBlock;
use crate::lines::Line;

/// Per-page statistics used as a weak supporting signal (not the primary
/// one) for heading detection.
#[derive(Debug, Clone, Copy)]
pub struct PageStats {
    /// Median line height across the page, in points.
    pub median_line_height: f64,
}

impl PageStats {
    /// Computes stats directly from a page's (rejoined) lines, before
    /// block grouping — this is what lets heading detection run on
    /// individual lines to decide block boundaries, rather than only
    /// after blocks already exist.
    pub fn from_lines(lines: &[Line]) -> Self {
        if lines.is_empty() {
            return Self { median_line_height: 10.0 };
        }
        let mut heights: Vec<f64> = lines.iter().map(|l| l.bbox.height()).collect();
        heights.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Self { median_line_height: heights[heights.len() / 2].max(1.0) }
    }
}

/// Score threshold at or above which a block is classified as a heading.
const HEADING_THRESHOLD: i32 = 4;

/// Decides whether `block` reads as a heading. Requires the block to be a
/// single line (multi-line blocks are always body text — no heading in
/// this corpus wraps across lines), then scores that line on:
/// - short (< 60 chars): +1
/// - no sentence-ending punctuation: +1
/// - title-case or ALL-CAPS: +2
/// - taller than the page's median line: +1
pub fn is_heading(block: &RawBlock, stats: PageStats) -> bool {
    if block.lines.len() != 1 {
        return false;
    }
    let text = block.lines[0].text();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    let mut score = 0;

    let len = trimmed.chars().count();
    if len > 0 && len < 60 {
        score += 1;
    }

    if !trimmed.ends_with(['.', ',', ';']) {
        score += 1;
    }

    let words: Vec<&str> = trimmed.split_whitespace().collect();
    let has_alpha = trimmed.chars().any(|c| c.is_alphabetic());
    let all_caps = has_alpha
        && trimmed
            .chars()
            .filter(|c| c.is_alphabetic())
            .all(|c| c.is_uppercase());
    // A word counts as "capitalized" by its first *alphabetic* character
    // (skipping any leading digits/punctuation, e.g. the "2" in
    // "2nd-level") being uppercase. Words with no alphabetic character at
    // all impose no constraint either way. Checking merely "not
    // lowercase" on the literal first character would wrongly treat a
    // digit-leading token like "2nd-level" as capitalized, since digits
    // are neither upper- nor lowercase -- exactly the false positive that
    // let a spell's "2nd-level illusion" line masquerade as a heading.
    let title_case = has_alpha
        && !words.is_empty()
        && words.iter().all(|w| match w.chars().find(|c| c.is_alphabetic()) {
            Some(c) => c.is_uppercase(),
            None => true,
        });
    if all_caps || title_case {
        score += 2;
    }

    if block.bbox.height() > stats.median_line_height * 1.05 {
        score += 1;
    }

    score >= HEADING_THRESHOLD
}

/// As [`is_heading`], scoring a bare [`Line`] rather than an already-built
/// [`RawBlock`]. Used before block grouping to decide which lines should
/// force their own block boundary (see `lib.rs`'s pipeline).
pub fn is_heading_line(line: &Line, stats: PageStats) -> bool {
    let block = RawBlock { bbox: line.bbox, lines: vec![line.clone()] };
    is_heading(&block, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lines::Line;
    use brain_core::{BBox, Word};

    fn single_line_block(text: &str, height: f64) -> RawBlock {
        let words: Vec<Word> = text
            .split_whitespace()
            .enumerate()
            .map(|(i, t)| Word {
                text: t.to_string(),
                bbox: BBox { x0: i as f64 * 20.0, y0: 0.0, x1: i as f64 * 20.0 + 15.0, y1: height },
                confidence: None,
            })
            .collect();
        let bbox = words[1..].iter().fold(words[0].bbox, |acc, w| acc.union(&w.bbox));
        RawBlock { bbox, lines: vec![Line { bbox, words }] }
    }

    #[test]
    fn short_title_case_line_is_a_heading() {
        let block = single_line_block("Minor Illusion", 8.0);
        let stats = PageStats { median_line_height: 7.0 };
        assert!(is_heading(&block, stats));
    }

    #[test]
    fn ordinary_sentence_is_not_a_heading() {
        let block = single_line_block("the spell also foils wish spells and effects.", 7.0);
        let stats = PageStats { median_line_height: 7.0 };
        assert!(!is_heading(&block, stats));
    }

    #[test]
    fn multiline_block_is_never_a_heading() {
        let mut block = single_line_block("Mirror Image", 8.0);
        block.lines.push(block.lines[0].clone());
        let stats = PageStats { median_line_height: 7.0 };
        assert!(!is_heading(&block, stats));
    }

    #[test]
    fn all_caps_monster_name_is_a_heading() {
        let block = single_line_block("URIDIMMU", 8.0);
        let stats = PageStats { median_line_height: 7.0 };
        assert!(is_heading(&block, stats));
    }
}
