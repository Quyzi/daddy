//! Paragraph (block) grouping: merges consecutive lines within a column
//! into blocks, splitting wherever the vertical gap between lines is
//! large relative to the column's typical line spacing — the same signal
//! a human eye uses to spot a paragraph break or a new heading.

use crate::lines::Line;
use brain_core::BBox;

/// A block is one or more consecutive lines with no unusually large gap
/// between them.
#[derive(Debug, Clone)]
pub struct RawBlock {
    /// Union bounding box of the block's lines.
    pub bbox: BBox,
    /// The block's lines, top to bottom.
    pub lines: Vec<Line>,
}

impl RawBlock {
    /// The block's text: lines joined by single newlines.
    /// The block reflowed as a single paragraph: lines joined by spaces,
    /// since a wrapped line break is a column-width artifact, not a
    /// sentence or paragraph boundary.
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.text())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    sorted[sorted.len() / 2]
}

/// Groups a column's lines (already sorted top-to-bottom by
/// [`crate::lines::group_lines`]) into blocks: consecutive lines with no
/// unusually large vertical gap between them, *and* no heading in
/// between. `force_break_before[i]` marks lines (typically ones
/// [`crate::headings::is_heading_line`] identified) that must start a new
/// block regardless of vertical spacing — this is what keeps a heading
/// from being swallowed into the paragraph that immediately follows it,
/// which plain gap-based grouping alone cannot guarantee.
pub fn group_into_blocks(lines: &[Line], force_break_before: &[bool]) -> Vec<RawBlock> {
    debug_assert_eq!(lines.len(), force_break_before.len());
    if lines.is_empty() {
        return Vec::new();
    }
    let heights: Vec<f64> = lines.iter().map(|l| l.bbox.height()).collect();
    let median_height = median(&heights).max(1.0);
    // A gap much larger than one line's height reads as a paragraph
    // break; 1.6x is generous enough to tolerate normal line-leading
    // variance while still catching real breaks.
    let gap_threshold = median_height * 1.6;

    let mut blocks = Vec::new();
    let mut current: Vec<Line> = vec![lines[0].clone()];
    for i in 1..lines.len() {
        let gap = lines[i].bbox.y0 - lines[i - 1].bbox.y1;
        // A heading always ends the block before it and starts its own:
        // force_break_before[i] covers "next line is a heading", and
        // force_break_before[i-1] (the line just added) covers "the line
        // we just closed out was itself a heading".
        let force_break = force_break_before[i] || force_break_before[i - 1];
        if gap > gap_threshold || force_break {
            blocks.push(make_block(std::mem::take(&mut current)));
        }
        current.push(lines[i].clone());
    }
    if !current.is_empty() {
        blocks.push(make_block(current));
    }
    blocks
}

fn make_block(lines: Vec<Line>) -> RawBlock {
    let bbox = lines[1..]
        .iter()
        .fold(lines[0].bbox, |acc, l| acc.union(&l.bbox));
    RawBlock { bbox, lines }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::Word;

    fn line_at(text: &str, y0: f64) -> Line {
        let words: Vec<Word> = text
            .split_whitespace()
            .enumerate()
            .map(|(i, t)| Word {
                text: t.to_string(),
                bbox: BBox { x0: i as f64 * 20.0, y0, x1: i as f64 * 20.0 + 15.0, y1: y0 + 10.0 },
                confidence: None,
            })
            .collect();
        let bbox = words[1..].iter().fold(words[0].bbox, |acc, w| acc.union(&w.bbox));
        Line { bbox, words }
    }

    #[test]
    fn splits_on_large_vertical_gaps_only() {
        let lines = vec![
            line_at("first paragraph line one", 0.0),
            line_at("first paragraph line two", 11.0),
            // big gap: new paragraph
            line_at("second paragraph starts here", 40.0),
            line_at("second paragraph continues", 51.0),
        ];
        let blocks = group_into_blocks(&lines, &[false, false, false, false]);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].lines.len(), 2);
        assert_eq!(blocks[1].lines.len(), 2);
    }
}
