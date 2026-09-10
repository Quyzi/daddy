//! Pure-geometry page reconstruction.
//!
//! Takes the flat, unordered word lists [`brain_extract`](../brain_extract/index.html)
//! backends produce ([`brain_core::RawPage`]) and rebuilds actual document
//! structure from them: which words form a word (OCR fragment rejoin),
//! which words form a line, which lines form a column, which blocks are
//! headings vs. body text, and which blocks are running chrome (headers/
//! footers) rather than content.
//!
//! This crate does no I/O and depends on nothing beyond `brain-core` and
//! `serde` — every stage is a pure function over in-memory geometry, which
//! is what makes it possible to test against golden fixtures
//! (`tests/fixtures/*.json`, real word data extracted from three
//! representative pages of this corpus) with no PDFs or external tools
//! involved.
//!
//! Pipeline, per page: [`columns::detect_columns`] -> [`lines::group_lines`]
//! -> page-wide word rejoin ([`rejoin::merge_threshold`] /
//! [`rejoin::page_fallback_threshold`] over every line's gaps) ->
//! [`headings::is_heading_line`] (decides forced block boundaries) ->
//! [`blocks::group_into_blocks`] -> [`headings::is_heading`] (final block
//! classification). Across a whole document, [`chrome::repeated_strings`]
//! then flags running headers/footers.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod blocks;
pub mod chrome;
pub mod columns;
pub mod headings;
pub mod lines;
pub mod rejoin;
mod util;

pub mod types;

pub use rejoin::looks_fragmented;
pub use types::{LaidOutBlock, LaidOutPage};

use blocks::group_into_blocks;
use brain_core::{BlockKind, RawPage};
use columns::detect_columns;
use headings::{is_heading, is_heading_line, PageStats};
use lines::{group_lines, Line};
use rejoin::{line_gaps, merge_threshold, merge_words, page_fallback_threshold};

/// Fraction of page height, measured from the top, considered a
/// candidate zone for a running header.
const CHROME_TOP_FRACTION: f64 = 0.12;
/// Fraction of page height, measured from the bottom, considered a
/// candidate zone for a running footer.
const CHROME_BOTTOM_FRACTION: f64 = 0.12;
/// A document shorter than this has too little signal for cross-page
/// chrome detection to be trustworthy, so [`reconstruct_document`] skips
/// it entirely (every block stays whatever [`layout_page`] classified it).
const MIN_PAGES_FOR_CHROME_DETECTION: usize = 3;

/// Reconstructs a single page's structure: columns, lines, rejoined
/// words, paragraph blocks, and heading classification. Does not detect
/// chrome — that requires whole-document context; use
/// [`reconstruct_document`] for that.
///
/// Word-fragment rejoin runs once, page-wide (see [`crate::rejoin`] for
/// why): line grouping happens first without merging fragments, every
/// intra-line gap on the page is pooled to compute a single merge
/// threshold, and only then are lines rejoined. Heading detection then
/// runs per-line (before block grouping) so a detected heading always
/// ends up alone in its own block, never fused with the paragraph that
/// follows it.
pub fn layout_page(raw: &RawPage) -> LaidOutPage {
    let columns = detect_columns(&raw.words, raw.width);
    let lines_per_column: Vec<Vec<Line>> = columns.iter().map(|words| group_lines(words)).collect();

    // Each line first tries to determine its own rejoin threshold from its
    // own gaps -- this is the reliable path and is what correctly rejoins
    // ordinary fragmented body paragraphs, which have plenty of gaps to
    // cluster. A short, heading-sized line often doesn't have enough
    // gaps to trust on its own; for those (and only on pages that
    // globally look fragmented in the first place) we fall back to the
    // median threshold that other, data-rich lines on the same page
    // already confirmed. A clean native-text page has no line confirm
    // any threshold at all, so the fallback is naturally `None` and
    // nothing on such a page is ever merged.
    let per_line_gaps: Vec<Vec<f64>> = lines_per_column
        .iter()
        .flatten()
        .map(|line| line_gaps(&line.words))
        .collect();
    let fallback = if looks_fragmented(&raw.words) {
        page_fallback_threshold(&per_line_gaps)
    } else {
        None
    };

    let rejoined_per_column: Vec<Vec<Line>> = lines_per_column
        .iter()
        .map(|lines| {
            lines
                .iter()
                .map(|line| {
                    let gaps = line_gaps(&line.words);
                    let threshold = merge_threshold(&gaps).or(fallback);
                    let words = merge_words(&line.words, threshold);
                    Line { bbox: line.bbox, words }
                })
                .collect()
        })
        .collect();

    let stats = PageStats::from_lines(
        &rejoined_per_column.iter().flatten().cloned().collect::<Vec<_>>(),
    );

    let mut laid_blocks = Vec::new();
    let mut ord = 0u32;
    for (col_idx, lines) in rejoined_per_column.into_iter().enumerate() {
        let heading_flags: Vec<bool> = lines.iter().map(|l| is_heading_line(l, stats)).collect();
        for block in group_into_blocks(&lines, &heading_flags) {
            let kind = if is_heading(&block, stats) { BlockKind::Heading } else { BlockKind::Body };
            laid_blocks.push(LaidOutBlock {
                col: col_idx as u32,
                ord,
                bbox: block.bbox,
                kind,
                text: block.text(),
            });
            ord += 1;
        }
    }

    LaidOutPage { page_no: raw.page_no, width: raw.width, height: raw.height, blocks: laid_blocks }
}

/// Reconstructs every page of a document and then strips running chrome
/// (headers/footers/page numbers that repeat, position-normalized, across
/// most pages) by retagging the matching blocks as [`BlockKind::Chrome`].
/// Chrome blocks are kept (for diagnostics) but excluded from
/// [`LaidOutPage::reading_order_text`] and should not be indexed.
pub fn reconstruct_document(raw_pages: &[RawPage]) -> Vec<LaidOutPage> {
    let mut pages: Vec<LaidOutPage> = raw_pages.iter().map(layout_page).collect();
    if pages.len() >= MIN_PAGES_FOR_CHROME_DETECTION {
        strip_chrome(&mut pages);
    }
    pages
}

/// Detects and retags running headers/footers across `pages`. Exposed
/// separately from [`reconstruct_document`] so callers who already have
/// `LaidOutPage`s (e.g. re-running chrome detection after an unrelated
/// change) don't need to re-run full layout.
pub fn strip_chrome(pages: &mut [LaidOutPage]) {
    let top_candidates: Vec<Option<String>> = pages
        .iter()
        .map(|p| top_candidate(p).map(|b| chrome::normalize(&b.text)))
        .collect();
    let bottom_candidates: Vec<Option<String>> = pages
        .iter()
        .map(|p| bottom_candidate(p).map(|b| chrome::normalize(&b.text)))
        .collect();

    let top_repeated = chrome::repeated_strings(&top_candidates);
    let bottom_repeated = chrome::repeated_strings(&bottom_candidates);

    for i in 0..pages.len() {
        let is_top_chrome = top_candidates[i].as_ref().is_some_and(|s| top_repeated.contains(s));
        let is_bottom_chrome = bottom_candidates[i].as_ref().is_some_and(|s| bottom_repeated.contains(s));
        let height = pages[i].height;
        if is_top_chrome {
            if let Some(b) = top_candidate_mut(&mut pages[i], height) {
                b.kind = BlockKind::Chrome;
            }
        }
        if is_bottom_chrome {
            if let Some(b) = bottom_candidate_mut(&mut pages[i], height) {
                b.kind = BlockKind::Chrome;
            }
        }
    }
}

fn top_candidate(page: &LaidOutPage) -> Option<&LaidOutBlock> {
    page.blocks
        .iter()
        .filter(|b| b.bbox.y0 <= page.height * CHROME_TOP_FRACTION)
        .min_by(|a, b| a.bbox.y0.partial_cmp(&b.bbox.y0).unwrap())
}

fn bottom_candidate(page: &LaidOutPage) -> Option<&LaidOutBlock> {
    page.blocks
        .iter()
        .filter(|b| b.bbox.y1 >= page.height * (1.0 - CHROME_BOTTOM_FRACTION))
        .max_by(|a, b| a.bbox.y1.partial_cmp(&b.bbox.y1).unwrap())
}

fn top_candidate_mut(page: &mut LaidOutPage, height: f64) -> Option<&mut LaidOutBlock> {
    page.blocks
        .iter_mut()
        .filter(|b| b.bbox.y0 <= height * CHROME_TOP_FRACTION)
        .min_by(|a, b| a.bbox.y0.partial_cmp(&b.bbox.y0).unwrap())
}

fn bottom_candidate_mut(page: &mut LaidOutPage, height: f64) -> Option<&mut LaidOutBlock> {
    page.blocks
        .iter_mut()
        .filter(|b| b.bbox.y1 >= height * (1.0 - CHROME_BOTTOM_FRACTION))
        .max_by(|a, b| a.bbox.y1.partial_cmp(&b.bbox.y1).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::{BBox, Word};

    /// Builds a synthetic single-column page: a running header at the
    /// very top, a body paragraph, and a page-number footer at the very
    /// bottom.
    fn page_with_chrome(page_no: u32, header: &str, body: &str, footer: &str) -> RawPage {
        let mut words = Vec::new();
        // Header near the very top, footer near the very bottom, body
        // solidly in the middle -- large gaps so these land in three
        // separate blocks, the way a real page's header/body/footer do.
        for (line, y0) in [(header, 5.0), (body, 400.0), (footer, 780.0)] {
            let mut x = 50.0;
            for tok in line.split_whitespace() {
                let w = tok.chars().count().max(1) as f64 * 6.0;
                words.push(Word {
                    text: tok.to_string(),
                    bbox: BBox { x0: x, y0, x1: x + w, y1: y0 + 10.0 },
                    confidence: None,
                });
                x += w + 4.0;
            }
        }
        RawPage { page_no, width: 600.0, height: 800.0, words, ocr_confidence: None }
    }

    #[test]
    fn reconstruct_document_strips_repeated_header_and_footer() {
        let raw_pages: Vec<RawPage> = (1..=5)
            .map(|n| {
                page_with_chrome(
                    n,
                    &format!("Curse of Strahd {n}"),
                    "The party enters the misty forest and finds an old church.",
                    &format!("{n}"),
                )
            })
            .collect();

        let pages = reconstruct_document(&raw_pages);
        assert_eq!(pages.len(), 5);

        for page in &pages {
            let chrome_blocks: Vec<&LaidOutBlock> =
                page.blocks.iter().filter(|b| b.kind == BlockKind::Chrome).collect();
            assert!(
                !chrome_blocks.is_empty(),
                "page {} should have had its repeated header/footer flagged as chrome",
                page.page_no
            );
            let text = page.reading_order_text();
            assert!(text.contains("misty forest"), "body text must survive: {text:?}");
            assert!(!text.contains("Curse of Strahd"), "chrome must be excluded from reading text: {text:?}");
        }
    }

    #[test]
    fn a_short_document_skips_chrome_detection_entirely() {
        // Only 2 pages: too little signal to trust repetition-based
        // chrome detection, so nothing should be retagged even though
        // the header text happens to repeat.
        let raw_pages: Vec<RawPage> = (1..=2)
            .map(|n| page_with_chrome(n, "Same Header", "Body text here.", "1"))
            .collect();
        let pages = reconstruct_document(&raw_pages);
        for page in &pages {
            assert!(page.blocks.iter().all(|b| b.kind != BlockKind::Chrome));
        }
    }
}
