//! Word rejoin: some OCR engines (notably the ABBYY/EPSON-scanned books in
//! this corpus) split single words into multiple fragments —
//! `"em otion s"` instead of `"emotions"`, sometimes down to individual
//! letter clusters for short heading-sized text. This module detects that
//! bimodal spacing pattern (intra-word gaps cluster tightly around ~1pt,
//! real inter-word gaps around ~3pt — see the corpus probing notes in the
//! implementation plan) and merges fragments back together.
//!
//! Fragmentation severity is a property of the whole page's scan (same
//! font, same OCR run), not of any one line — and a short heading line
//! may have only 2-3 gaps, far too few to cluster reliably on its own.
//! So the threshold is computed once from every intra-line gap on a page
//! ([`merge_threshold`] over [`line_gaps`] collected across all lines),
//! then applied uniformly via [`merge_words`]. [`rejoin_line`] remains as
//! a convenience wrapper (compute-then-apply on a single line) for
//! callers, such as tests, that don't need page-wide aggregation.
//!
//! Native PDF text layers have no bimodal gap pattern — their gaps are
//! all "real" word spacing — so [`merge_threshold`] returns `None` when
//! it doesn't see two well-separated clusters, rather than guessing.

use crate::util::kmeans_1d_2;
use brain_core::Word;

/// Heuristic pre-check: does this page look like it went through
/// letter/fragment-splitting OCR at all? Real English prose averages
/// around 4.7 characters per word and has only a modest share of
/// genuinely one- or two-letter tokens ("a", "of", "to", ...); the
/// ABBYY/EPSON-scanned books in this corpus average under 4 characters
/// per *fragment* with roughly twice the rate of 1-2 character tokens
/// (measured directly on this crate's golden fixtures: ~3.6 avg length /
/// 43% short on a fragmented page vs. ~4.9 / ~20% on clean native-text
/// pages). Gating on this before ever computing a merge threshold is what
/// keeps normal native-text pages — which can have their own, unrelated,
/// mild gap bimodality from kerning or punctuation — from being
/// incorrectly merged into one run-on word.
pub fn looks_fragmented(words: &[Word]) -> bool {
    if words.len() < 20 {
        return false;
    }
    let total = words.len() as f64;
    let avg_len = words.iter().map(|w| w.text.chars().count()).sum::<usize>() as f64 / total;
    let short_fraction =
        words.iter().filter(|w| w.text.chars().count() <= 2).count() as f64 / total;
    avg_len < 4.2 && short_fraction > 0.30
}

/// Returns the horizontal gaps between consecutive words in `words`,
/// which must already be sorted left-to-right by `x0` (as
/// [`crate::lines::group_lines`] guarantees for each line it produces).
pub fn line_gaps(words: &[Word]) -> Vec<f64> {
    words
        .windows(2)
        .map(|w| (w[1].bbox.x0 - w[0].bbox.x1).max(0.0))
        .collect()
}

/// Decides the gap width below which two adjacent words should be merged
/// into one, or `None` if `gaps` shows no clear bimodal split (the common
/// case for native, non-fragmented text — merge nothing).
pub fn merge_threshold(gaps: &[f64]) -> Option<f64> {
    // A handful of gaps (a short heading-sized line) is too easy to
    // cluster "successfully" by coincidence rather than by genuine
    // bimodality -- e.g. 3 points can trivially split 2-vs-1 in a way
    // that happens to pass the separation check below without actually
    // reflecting two real populations. Requiring more data before
    // trusting a line's *own* clustering is what makes such lines defer
    // to `page_fallback_threshold` instead.
    const MIN_GAPS_FOR_OWN_THRESHOLD: usize = 6;
    if gaps.len() < MIN_GAPS_FOR_OWN_THRESHOLD {
        return None;
    }
    let (a, b) = kmeans_1d_2(gaps);
    let (lo, hi) = (a.min(b), a.max(b));
    // Require the clusters to actually be separated (not just noise
    // around one real spacing value) before trusting the split.
    if hi < 1e-6 || hi / lo.max(0.05) < 1.8 {
        return None;
    }
    Some((lo + hi) / 2.0)
}

/// Merges words already sorted left-to-right by `x0` wherever the gap
/// between them is at or below `threshold`. Passing `threshold: None`
/// (no bimodal split detected anywhere on the page) returns `words`
/// unchanged.
pub fn merge_words(words: &[Word], threshold: Option<f64>) -> Vec<Word> {
    let threshold = match threshold {
        Some(t) => t,
        None => return words.to_vec(),
    };
    if words.len() <= 1 {
        return words.to_vec();
    }
    let gaps = line_gaps(words);
    let mut out: Vec<Word> = Vec::with_capacity(words.len());
    let mut current = words[0].clone();
    for (i, gap) in gaps.iter().enumerate() {
        let next = &words[i + 1];
        if *gap <= threshold {
            current.text.push_str(&next.text);
            current.bbox = current.bbox.union(&next.bbox);
            current.confidence = combine_confidence(current.confidence, next.confidence);
        } else {
            out.push(current);
            current = next.clone();
        }
    }
    out.push(current);
    out
}

/// Computes a page-wide fallback merge threshold: runs per-line
/// clustering independently on every line's gaps and takes the *median*
/// of the thresholds that succeeded. A short, heading-sized line often
/// has too few gaps for [`merge_threshold`] to trust on its own (that
/// early-return exists precisely to avoid guessing from 2-3 data
/// points) — this lets such a line borrow the split point that longer,
/// data-rich lines on the *same* page already confirmed, rather than
/// being left unmerged just because it happened to be short.
///
/// Returns `None` when no line on the page independently confirmed a
/// bimodal split — which is exactly what happens on clean native-text
/// pages, so this is naturally a no-op there (see `lib.rs`'s pipeline,
/// which additionally gates this behind [`looks_fragmented`] as a second,
/// page-level safety check).
pub fn page_fallback_threshold(per_line_gaps: &[Vec<f64>]) -> Option<f64> {
    let mut confirmed: Vec<f64> = per_line_gaps.iter().filter_map(|g| merge_threshold(g)).collect();
    if confirmed.is_empty() {
        return None;
    }
    confirmed.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(confirmed[confirmed.len() / 2])
}

/// Convenience wrapper: sorts `words` by `x0`, computes a threshold from
/// this line alone, and merges. Prefer [`merge_words`] with a
/// page-wide-aggregated threshold when more than a handful of words are
/// available — see the module docs for why.
pub fn rejoin_line(words: &[Word]) -> Vec<Word> {
    if words.len() <= 1 {
        return words.to_vec();
    }
    let mut sorted = words.to_vec();
    sorted.sort_by(|a, b| a.bbox.x0.partial_cmp(&b.bbox.x0).unwrap());
    let threshold = merge_threshold(&line_gaps(&sorted));
    merge_words(&sorted, threshold)
}

fn combine_confidence(a: Option<f32>, b: Option<f32>) -> Option<f32> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::BBox;

    fn w(text: &str, x0: f64, x1: f64) -> Word {
        Word { text: text.into(), bbox: BBox { x0, y0: 0.0, x1, y1: 10.0 }, confidence: None }
    }

    #[test]
    fn rejoins_fragmented_ocr_words_but_keeps_real_word_gaps() {
        // "em|otion|s" "or" "re|a|d" "its" -- enough gaps (7) to clear
        // merge_threshold's own-line minimum sample size.
        let words = vec![
            w("em", 0.0, 10.0),
            w("otion", 11.0, 30.0), // gap 1.0 (intra-word)
            w("s", 31.0, 35.0),     // gap 1.0 (intra-word)
            w("or", 38.0, 45.0),    // gap 3.0 (real space)
            w("re", 48.0, 58.0),    // gap 3.0 (real space)
            w("a", 59.0, 63.0),     // gap 1.0 (intra-word)
            w("d", 64.0, 68.0),     // gap 1.0 (intra-word)
            w("its", 71.0, 85.0),   // gap 3.0 (real space)
        ];
        let out = rejoin_line(&words);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].text, "emotions");
        assert_eq!(out[1].text, "or");
        assert_eq!(out[2].text, "read");
        assert_eq!(out[3].text, "its");
    }

    #[test]
    fn leaves_normal_native_text_untouched() {
        // Uniform ~3pt gaps everywhere: no bimodal split, nothing merges.
        let words = vec![
            w("the", 0.0, 15.0),
            w("quick", 18.0, 45.0),
            w("brown", 48.0, 75.0),
            w("fox", 78.0, 92.0),
        ];
        let out = rejoin_line(&words);
        assert_eq!(out.len(), 4);
        assert_eq!(out[1].text, "quick");
    }

    #[test]
    fn single_word_line_is_a_no_op() {
        let words = vec![w("Hello", 0.0, 20.0)];
        let out = rejoin_line(&words);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "Hello");
    }

    #[test]
    fn looks_fragmented_separates_real_corpus_shapes() {
        // Ratios measured directly on this crate's golden fixtures.
        let fragmented = synth_words(3.59, 0.43, 1106);
        let native_a = synth_words(4.86, 0.21, 651);
        let native_b = synth_words(4.91, 0.18, 474);
        assert!(looks_fragmented(&fragmented));
        assert!(!looks_fragmented(&native_a));
        assert!(!looks_fragmented(&native_b));
    }

    /// Builds a word list with an approximate average length and a given
    /// fraction of <=2-char "words", for exercising [`looks_fragmented`]
    /// without needing real fixture files in this unit test.
    fn synth_words(avg_len: f64, short_fraction: f64, n: usize) -> Vec<Word> {
        let n_short = (n as f64 * short_fraction).round() as usize;
        let remaining_len_total = avg_len * n as f64 - n_short as f64 * 2.0;
        let long_len = ((remaining_len_total / (n - n_short) as f64).round() as usize).max(3);
        (0..n)
            .map(|i| {
                let len = if i < n_short { 2 } else { long_len };
                w(&"x".repeat(len), 0.0, len as f64)
            })
            .collect()
    }

    #[test]
    fn a_short_line_borrows_a_page_wide_threshold_it_could_not_derive_alone() {
        // Only 2 gaps here -- too few for merge_threshold to trust on its
        // own -- but a threshold computed from a larger page-wide sample
        // (passed in explicitly) still merges it correctly.
        let words = vec![w("M", 0.0, 5.0), w("irage", 6.0, 30.0), w("A", 33.0, 38.0), w("rcane", 39.0, 60.0)];
        let page_wide_threshold = Some(2.0); // e.g. derived from hundreds of page gaps
        let sorted = {
            let mut s = words.clone();
            s.sort_by(|a, b| a.bbox.x0.partial_cmp(&b.bbox.x0).unwrap());
            s
        };
        let out = merge_words(&sorted, page_wide_threshold);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "Mirage");
        assert_eq!(out[1].text, "Arcane");
    }
}
