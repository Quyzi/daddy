//! Column detection: splits a page's words into left-to-right reading
//! columns by finding a vertical "gutter" — a contiguous band of the page
//! width that no word's bounding box ever covers, running most of the
//! page's height.
//!
//! This looks for a real empty gap rather than clustering directly on
//! word `x0` values, and that distinction matters: within one column of
//! justified or ragged-right prose, individual *word* left edges scatter
//! across that column's entire width (only line/paragraph *starts* sit at
//! the margin), so a page of ordinary single-column text can present a
//! wide, high-variance spread of `x0` values that naive 2-means
//! clustering will happily bisect into a bogus "second column" even
//! though no real gutter exists there. A genuine two-column layout, by
//! contrast, leaves a real gutter that words essentially never cover —
//! that's the signal this module trusts, measured two ways against real
//! pages in this corpus: the search area is bounded to strictly *inside*
//! the page's own occupied width (so a wide leading/trailing page margin
//! is never mistaken for a gutter just because it's wider than the real
//! one), and a handful of stray intruding words — a dropped cap, a
//! justification quirk — are tolerated rather than requiring literally
//! zero coverage.

use brain_core::Word;

/// Number of histogram bins across the page width. Fine enough to find
/// gutters as narrow as a few points without being sensitive to
/// individual glyph-level bbox noise.
const BINS: usize = 200;

/// A bin counts as "gutter-eligible" with this many or fewer word
/// bounding boxes covering it. Real gutters in this corpus aren't always
/// perfectly zero-density (occasional kerning/justification puts a stray
/// glyph a point or two into the gap), so tolerating a small count finds
/// them without also finding gaps inside genuinely dense single-column
/// text (which this threshold is far too low to bridge).
const LOW_DENSITY_MAX: usize = 2;

/// Minimum gutter width, as a fraction of page width, before it's judged
/// a real column gap rather than an incidental short-line gap.
const MIN_GUTTER_FRACTION: f64 = 0.02;

/// Minimum fraction of a page's words that must fall in the smaller
/// column before we trust a two-column split (guards against a single
/// title line at a different `x0` looking like a "column").
const MIN_CLUSTER_FRACTION: f64 = 0.1;

/// Minimum vertical span each candidate column must cover, as a fraction
/// of the page's overall word y-range, before we trust a two-column
/// split. Without this, a single "running title | page number" header
/// line — two short runs of text at very different `x0` but nearly
/// identical `y0` — looks exactly like a two-column split by x-position
/// alone, even on an otherwise single-column page. Real body columns run
/// most of the page's height; a header artifact doesn't.
const MIN_Y_SPAN_FRACTION: f64 = 0.3;

/// Splits `words` into 1 or 2 columns, returned left-to-right. Each
/// output group preserves the relative order of `words`; callers sort
/// within a column separately.
pub fn detect_columns(words: &[Word], page_width: f64) -> Vec<Vec<Word>> {
    if words.len() < 8 || page_width <= 0.0 {
        return vec![words.to_vec()];
    }

    let split_x = match find_gutter(words, page_width) {
        Some(x) => x,
        None => return vec![words.to_vec()],
    };

    let mut left = Vec::new();
    let mut right = Vec::new();
    for w in words {
        if w.bbox.x0 < split_x {
            left.push(w.clone());
        } else {
            right.push(w.clone());
        }
    }

    let total = words.len() as f64;
    let smaller_fraction = (left.len().min(right.len()) as f64) / total;
    if smaller_fraction < MIN_CLUSTER_FRACTION {
        return vec![words.to_vec()];
    }

    let (page_y_min, page_y_max) = y_range(words);
    let page_y_span = (page_y_max - page_y_min).max(1.0);
    let (left_min, left_max) = y_range(&left);
    let (right_min, right_max) = y_range(&right);
    let left_spans_page = (left_max - left_min) / page_y_span >= MIN_Y_SPAN_FRACTION;
    let right_spans_page = (right_max - right_min) / page_y_span >= MIN_Y_SPAN_FRACTION;
    if !left_spans_page || !right_spans_page {
        return vec![words.to_vec()];
    }

    vec![left, right]
}

/// Finds the widest contiguous low-density x-band strictly inside the
/// page's own occupied width and, if it's wide enough to count as a real
/// gutter, returns its midpoint as the column split point.
fn find_gutter(words: &[Word], page_width: f64) -> Option<f64> {
    let bin_width = page_width / BINS as f64;
    let mut density = vec![0usize; BINS];
    for w in words {
        let b0 = ((w.bbox.x0 / bin_width) as usize).min(BINS - 1);
        let b1 = ((w.bbox.x1 / bin_width) as usize).min(BINS - 1);
        for count in density.iter_mut().take(b1 + 1).skip(b0) {
            *count += 1;
        }
    }

    // Bound the search to strictly inside where content actually is: a
    // page's leading/trailing margin is often wider than its real
    // inter-column gutter, and would otherwise win as the "widest empty
    // run" every time.
    let first_occupied = density.iter().position(|&c| c > 0);
    let last_occupied = density.iter().rposition(|&c| c > 0);
    let (Some(first), Some(last)) = (first_occupied, last_occupied) else {
        return None;
    };
    if last <= first {
        return None;
    }

    let mut best: Option<(usize, usize)> = None;
    let mut run_start: Option<usize> = None;
    for (i, &count) in density.iter().enumerate().take(last).skip(first + 1) {
        if count <= LOW_DENSITY_MAX {
            run_start.get_or_insert(i);
        } else if let Some(s) = run_start.take() {
            consider_run(&mut best, s, i);
        }
    }
    if let Some(s) = run_start {
        consider_run(&mut best, s, last);
    }

    let (s, e) = best?;
    let min_gutter_bins = ((BINS as f64) * MIN_GUTTER_FRACTION).ceil() as usize;
    if e - s < min_gutter_bins.max(1) {
        return None;
    }
    Some(((s + e) as f64 / 2.0) * bin_width)
}

fn consider_run(best: &mut Option<(usize, usize)>, start: usize, end: usize) {
    let len = end - start;
    if best.map(|(s, e)| e - s).unwrap_or(0) < len {
        *best = Some((start, end));
    }
}

fn y_range(words: &[Word]) -> (f64, f64) {
    let min = words.iter().map(|w| w.bbox.y0).fold(f64::INFINITY, f64::min);
    let max = words.iter().map(|w| w.bbox.y1).fold(f64::NEG_INFINITY, f64::max);
    (min, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::BBox;

    fn word_at(x0: f64, y0: f64) -> Word {
        Word {
            text: "w".into(),
            bbox: BBox { x0, y0, x1: x0 + 20.0, y1: y0 + 10.0 },
            confidence: None,
        }
    }

    #[test]
    fn splits_a_clear_two_column_page() {
        // Mirrors the PHB p239 probing: left column ~x0=100, right ~x0=350,
        // page width ~615, with a wide empty gutter between them.
        let mut words = Vec::new();
        for i in 0..20 {
            words.push(word_at(100.0 + (i % 3) as f64, i as f64 * 12.0));
        }
        for i in 0..20 {
            words.push(word_at(350.0 + (i % 3) as f64, i as f64 * 12.0));
        }
        let cols = detect_columns(&words, 615.0);
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].len(), 20);
        assert_eq!(cols[1].len(), 20);
        assert!(cols[0][0].bbox.x0 < cols[1][0].bbox.x0);
    }

    #[test]
    fn keeps_a_narrow_single_column_page_as_one_group() {
        let words: Vec<Word> = (0..20).map(|i| word_at(100.0 + (i as f64 * 5.0) % 40.0, i as f64 * 12.0)).collect();
        let cols = detect_columns(&words, 615.0);
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].len(), 20);
    }

    #[test]
    fn keeps_a_full_width_single_column_page_as_one_group() {
        // Realistic single-column prose: word x0 scattered across nearly
        // the whole text width (as real mid-line words are, not just
        // line starts), with no point in the middle left permanently
        // uncovered by every line. This is the shape that defeated a
        // naive "2-means on every word's x0" approach: such clustering
        // has no trouble bisecting a wide, continuous spread into two
        // bogus halves even though no real gutter exists.
        let mut words = Vec::new();
        for line in 0..30 {
            let y = line as f64 * 12.0;
            // 12 words per line, x0 spread fairly evenly across ~50..380
            // on a 430-wide page, each line phase-shifted slightly so no
            // single x column is left uncovered by every line.
            for word_idx in 0..12 {
                let x0 = 50.0 + (word_idx as f64) * 27.0 + ((line % 5) as f64) * 3.0;
                words.push(word_at(x0, y));
            }
        }
        let cols = detect_columns(&words, 430.0);
        assert_eq!(cols.len(), 1, "no real gutter exists; word x0 spread alone must not fake one");
    }

    #[test]
    fn ignores_a_running_header_split_on_an_otherwise_single_column_page() {
        // A "running title | page number" header sits at two very
        // different x0 but the *same* y0 -- exactly the pattern that
        // fooled an earlier version of this detector on a real
        // single-column narrative book in this corpus. Body text below
        // it is single-column and spans most of the page.
        let mut words: Vec<Word> = (0..20).map(|i| word_at(50.0 + (i % 3) as f64, i as f64 * 12.0)).collect();
        for i in 0..4 {
            words.push(word_at(300.0 + i as f64, 0.0)); // header fragment, y0=0 only
        }
        let cols = detect_columns(&words, 400.0);
        assert_eq!(cols.len(), 1, "a header artifact confined to one y-band must not create a fake column");
    }

    #[test]
    fn ignores_a_lone_outlier_like_a_full_width_title() {
        let mut words: Vec<Word> = (0..30).map(|i| word_at(100.0 + (i % 3) as f64, i as f64 * 12.0)).collect();
        words.push(word_at(400.0, 0.0)); // a single stray word, not a real column
        let cols = detect_columns(&words, 615.0);
        assert_eq!(cols.len(), 1, "one outlier word must not be treated as a second column");
    }
}
