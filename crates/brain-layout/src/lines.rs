//! Line assembly: groups words within a single column into visual lines
//! using vertical position and sorts each line left-to-right. Word
//! fragments are *not* rejoined here — see [`crate::rejoin`] and the
//! top-level pipeline in `lib.rs` for why that happens as a separate,
//! whole-page pass.

use brain_core::{BBox, Word};

/// One assembled line of text, words sorted left-to-right. Word
/// fragments have not yet been rejoined (see the module docs).
#[derive(Debug, Clone)]
pub struct Line {
    /// Union bounding box of the line's words.
    pub bbox: BBox,
    /// Words, sorted left-to-right, not yet rejoined.
    pub words: Vec<Word>,
}

impl Line {
    /// The line's text, words joined by single spaces.
    pub fn text(&self) -> String {
        self.words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Groups `words` (already restricted to one column) into lines. A new
/// word joins the current line if its vertical center falls within the
/// current line's height envelope; otherwise it starts a new line. Using
/// each line's *first* word as the fixed vertical reference (rather than
/// an accumulating union) avoids slow drift merging genuinely separate
/// lines on a page with slightly sloped OCR baselines.
pub fn group_lines(words: &[Word]) -> Vec<Line> {
    if words.is_empty() {
        return Vec::new();
    }
    let mut sorted = words.to_vec();
    sorted.sort_by(|a, b| a.bbox.y0.partial_cmp(&b.bbox.y0).unwrap());

    let mut raw_lines: Vec<Vec<Word>> = Vec::new();
    let mut current: Vec<Word> = vec![sorted[0].clone()];
    let mut ref_center = sorted[0].bbox.y_center();
    let mut ref_height = sorted[0].bbox.height().max(1.0);

    for w in &sorted[1..] {
        let half = (ref_height.max(w.bbox.height().max(1.0))) * 0.6;
        if (w.bbox.y_center() - ref_center).abs() <= half {
            current.push(w.clone());
        } else {
            raw_lines.push(std::mem::take(&mut current));
            ref_center = w.bbox.y_center();
            ref_height = w.bbox.height().max(1.0);
            current.push(w.clone());
        }
    }
    raw_lines.push(current);

    raw_lines
        .into_iter()
        .map(|mut line_words| {
            line_words.sort_by(|a, b| a.bbox.x0.partial_cmp(&b.bbox.x0).unwrap());
            let bbox = line_words[1..]
                .iter()
                .fold(line_words[0].bbox, |acc, w| acc.union(&w.bbox));
            Line { bbox, words: line_words }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::BBox;

    fn word_at(text: &str, x0: f64, y0: f64) -> Word {
        Word { text: text.into(), bbox: BBox { x0, y0, x1: x0 + 15.0, y1: y0 + 10.0 }, confidence: None }
    }

    #[test]
    fn groups_words_by_vertical_band_into_ordered_lines() {
        let words = vec![
            word_at("world", 20.0, 0.0),
            word_at("hello", 0.0, 0.0),
            word_at("line", 0.0, 12.0),
            word_at("second", 20.0, 12.0),
        ];
        let lines = group_lines(&words);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text(), "hello world");
        assert_eq!(lines[1].text(), "line second");
    }
}
