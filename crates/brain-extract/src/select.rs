//! Automatic backend selection: prefer a document's native PDF text and
//! fall back to OCR only for pages whose text layer is too thin to be
//! useful — the corpus probing in the implementation plan found this
//! simple character-count heuristic cleanly separates the ~47 books with
//! real text layers from the ~10 that are pure image scans, with zero
//! false positives, so no more elaborate signal (e.g. shelling out to
//! `pdfimages -list`) is needed here.

use crate::poppler::PopplerExtractor;
use crate::tesseract::TesseractExtractor;
use crate::types::{Extractor, PageRange};
use brain_core::error::Result;
use brain_core::{BrainConfig, OcrMode, RawPage};
use std::path::Path;

/// Extracts every page of `path`, honoring `config.ocr`:
/// - [`OcrMode::Never`]: native text only, however thin.
/// - [`OcrMode::Always`]: OCR every page, ignoring native text.
/// - [`OcrMode::Auto`] (default): native text first; any page with fewer
///   than `config.thin_text_chars` characters is re-extracted via OCR and
///   the OCR result replaces it.
pub fn extract_auto(
    path: &Path,
    poppler: &PopplerExtractor,
    tesseract: &TesseractExtractor,
    config: &BrainConfig,
) -> Result<Vec<RawPage>> {
    match config.ocr {
        OcrMode::Always => tesseract.extract(path, PageRange::All),
        OcrMode::Never => poppler.extract(path, PageRange::All),
        OcrMode::Auto => {
            let mut pages = poppler.extract(path, PageRange::All)?;
            let thin: Vec<u32> = pages
                .iter()
                .filter(|p| p.char_count() < config.thin_text_chars)
                .map(|p| p.page_no)
                .collect();
            if thin.is_empty() {
                return Ok(pages);
            }
            // OCR exactly the thin pages, not the span from the lowest
            // to the highest one -- a handful of scattered image-heavy
            // pages in an otherwise-fine book must not drag the whole
            // book between them through OCR (see `extract_pages`'s docs).
            let ocred = tesseract.extract_pages(path, &thin)?;
            for page in ocred {
                if let Some(slot) = pages.iter_mut().find(|p| p.page_no == page.page_no) {
                    *slot = page;
                }
            }
            Ok(pages)
        }
    }
}

/// Whether a page's extracted text is unreliable enough to exclude from
/// indexing: an OCR'd page below `config.min_ocr_confidence`, or a
/// never-OCR'd page whose native text is still thinner than
/// `config.thin_text_chars` (e.g. `OcrMode::Never` on a scanned book, or a
/// genuinely blank/image-only page OCR found nothing on).
pub fn is_low_confidence(page: &RawPage, config: &BrainConfig) -> bool {
    match page.ocr_confidence {
        Some(conf) => conf < config.min_ocr_confidence,
        None => page.char_count() < config.thin_text_chars,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::Word;

    fn word(text: &str) -> Word {
        Word {
            text: text.to_string(),
            bbox: brain_core::BBox { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 },
            confidence: None,
        }
    }

    #[test]
    fn low_confidence_flags_thin_never_ocred_pages() {
        let config = BrainConfig::default();
        let page = RawPage { page_no: 1, width: 1.0, height: 1.0, words: vec![], ocr_confidence: None };
        assert!(is_low_confidence(&page, &config));

        let page = RawPage {
            page_no: 1,
            width: 1.0,
            height: 1.0,
            words: (0..200).map(|_| word("word")).collect(),
            ocr_confidence: None,
        };
        assert!(!is_low_confidence(&page, &config));
    }

    #[test]
    fn low_confidence_flags_poor_ocr_regardless_of_char_count() {
        let config = BrainConfig::default();
        let page = RawPage {
            page_no: 1,
            width: 1.0,
            height: 1.0,
            words: (0..200).map(|_| word("word")).collect(),
            ocr_confidence: Some(10.0),
        };
        assert!(is_low_confidence(&page, &config));
    }
}
