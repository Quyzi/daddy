//! OCR extraction backend: renders PDF pages (via poppler's `pdftoppm`) or
//! reads standalone images directly, then runs `tesseract` in TSV mode to
//! recover per-word bounding boxes *and* confidence scores — the
//! confidence is what lets low-quality scans be flagged and excluded from
//! indexing rather than silently poisoning the graph.

use crate::pdfutil;
use crate::types::{Capability, Extractor, PageRange};
use brain_core::error::{BrainError, Result};
use brain_core::{BBox, RawPage, Word};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "tif", "tiff", "bmp"];

/// Default render resolution for PDF pages before OCR. 300 dpi is the
/// standard sweet spot for tesseract accuracy vs. render/OCR time; the
/// corpus probing in the implementation plan found the embedded scan
/// images in this corpus are only 131-150 ppi, so re-rendering at 300 dpi
/// (rather than extracting the embedded image directly) meaningfully
/// improves OCR quality.
pub const DEFAULT_DPI: u32 = 300;

/// OCRs PDF pages and standalone images via `pdftoppm` + `tesseract`.
pub struct TesseractExtractor {
    pdftoppm_bin: PathBuf,
    pdfinfo_bin: PathBuf,
    tesseract_bin: PathBuf,
    dpi: u32,
}

impl TesseractExtractor {
    /// Locates `pdftoppm`, `pdfinfo`, and `tesseract` on `PATH` at the
    /// default DPI.
    pub fn new() -> Result<Self> {
        Self::with_dpi(DEFAULT_DPI)
    }

    /// As [`TesseractExtractor::new`], rendering PDF pages at `dpi`.
    pub fn with_dpi(dpi: u32) -> Result<Self> {
        let pdftoppm_bin = which::which("pdftoppm")
            .map_err(|_| BrainError::Extraction("pdftoppm not found on PATH (install poppler-utils)".into()))?;
        let pdfinfo_bin = pdfutil::locate_pdfinfo()?;
        let tesseract_bin = which::which("tesseract")
            .map_err(|_| BrainError::Extraction("tesseract not found on PATH (install tesseract-ocr)".into()))?;
        Ok(Self { pdftoppm_bin, pdfinfo_bin, tesseract_bin, dpi })
    }

    /// Renders one PDF page to a temporary PNG and OCRs it.
    fn ocr_pdf_page(&self, path: &Path, page_no: u32) -> Result<RawPage> {
        let dir = tempfile::Builder::new()
            .prefix("brain-ocr-")
            .tempdir()
            .map_err(BrainError::Io)?;
        let prefix = dir.path().join("page");

        let status = Command::new(&self.pdftoppm_bin)
            .arg("-r")
            .arg(self.dpi.to_string())
            .arg("-png")
            .arg("-f")
            .arg(page_no.to_string())
            .arg("-l")
            .arg(page_no.to_string())
            .arg(path)
            .arg(&prefix)
            .status()
            .map_err(|e| BrainError::Extraction(format!("running pdftoppm: {e}")))?;
        if !status.success() {
            return Err(BrainError::Extraction(format!(
                "pdftoppm failed rendering page {page_no} of {}",
                path.display()
            )));
        }

        let png = std::fs::read_dir(dir.path())
            .map_err(BrainError::Io)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.extension().and_then(|e| e.to_str()) == Some("png"))
            .ok_or_else(|| {
                BrainError::Extraction(format!(
                    "pdftoppm produced no image for page {page_no} of {}",
                    path.display()
                ))
            })?;

        let mut page = self.ocr_image(&png)?;
        page.page_no = page_no;
        Ok(page)
    }

    /// Runs tesseract TSV mode on an already-rendered image file and
    /// converts pixel coordinates to page points (assuming the image was
    /// rendered at `self.dpi`).
    fn ocr_image(&self, image: &Path) -> Result<RawPage> {
        let output = Command::new(&self.tesseract_bin)
            // Tesseract multi-threads internally (OpenMP) by default. We
            // already parallelize across pages with rayon, one process
            // per page, so without this every process would *also* try to
            // spawn a full thread pool — oversubscribing an 8-core box
            // 8x-over and making each page take a minute instead of a
            // second. Pin each subprocess to one thread and let rayon own
            // all the parallelism.
            .env("OMP_THREAD_LIMIT", "1")
            .arg(image)
            .arg("stdout")
            .arg("--psm")
            .arg("3")
            .arg("tsv")
            .output()
            .map_err(|e| BrainError::Extraction(format!("running tesseract: {e}")))?;
        if !output.status.success() {
            return Err(BrainError::Extraction(format!(
                "tesseract failed on {}: {}",
                image.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let tsv = String::from_utf8_lossy(&output.stdout);
        Ok(parse_tsv(&tsv, self.dpi))
    }
}

impl Extractor for TesseractExtractor {
    fn name(&self) -> &'static str {
        "tesseract"
    }

    fn probe(&self, path: &Path) -> Result<Capability> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("pdf") => match pdfutil::page_count(&self.pdfinfo_bin, path)? {
                Some(n) => Ok(Capability { supported: true, page_count: Some(n), note: None }),
                None => Ok(Capability { supported: false, page_count: None, note: Some("unreadable PDF".into()) }),
            },
            Some(e) if IMAGE_EXTENSIONS.contains(&e) => {
                Ok(Capability { supported: true, page_count: Some(1), note: None })
            }
            _ => Ok(Capability { supported: false, page_count: None, note: Some("not a PDF or image".into()) }),
        }
    }

    fn extract(&self, path: &Path, pages: PageRange) -> Result<Vec<RawPage>> {
        let is_pdf = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false);

        if !is_pdf {
            return Ok(vec![self.ocr_image(path)?]);
        }

        let (start, end) = pdfutil::resolve_range(&self.pdfinfo_bin, path, pages)?;
        self.extract_pages(path, &(start..=end).collect::<Vec<_>>())
    }
}

impl TesseractExtractor {
    /// OCRs an arbitrary, not-necessarily-contiguous set of page numbers
    /// from one PDF, in parallel across all cores.
    ///
    /// This exists separately from [`Extractor::extract`] (which only
    /// takes a contiguous [`PageRange`]) because [`crate::select::extract_auto`]'s
    /// "auto" mode identifies *specific* thin pages scattered through an
    /// otherwise fine document — a handful of full-page illustrations in
    /// a 400-page rulebook, say. Extracting `PageRange::Range(lo, hi)`
    /// spanning the lowest to highest such page would OCR the entire
    /// span between them, including hundreds of pages that never needed
    /// it; calling this instead with the exact scattered page list keeps
    /// the OCR workload proportional to how much of the book actually
    /// needs it, while still parallelizing fully across every page that
    /// does (this was measured to turn a multi-minute hang on a mostly-clean
    /// book into a sub-second no-op).
    pub fn extract_pages(&self, path: &Path, page_numbers: &[u32]) -> Result<Vec<RawPage>> {
        let mut results: Vec<(u32, Result<RawPage>)> = page_numbers
            .par_iter()
            .map(|&n| (n, self.ocr_pdf_page(path, n)))
            .collect();
        results.sort_by_key(|(n, _)| *n);
        results.into_iter().map(|(_, r)| r).collect()
    }
}

/// Parses tesseract's `tsv` output format into a [`RawPage`]. Level 1 is
/// the whole-image row (used for page dimensions); level 5 rows are
/// individual words.
fn parse_tsv(tsv: &str, dpi: u32) -> RawPage {
    let scale = 72.0 / dpi as f64; // pixels (at `dpi`) -> PDF points
    let mut width = 0.0;
    let mut height = 0.0;
    let mut words = Vec::new();
    let mut confidences = Vec::new();

    for line in tsv.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 {
            continue;
        }
        let level: i32 = cols[0].parse().unwrap_or(0);
        if level == 1 {
            width = cols[8].parse::<f64>().unwrap_or(0.0) * scale;
            height = cols[9].parse::<f64>().unwrap_or(0.0) * scale;
        } else if level == 5 {
            let text = cols[11..].join("\t");
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            let left: f64 = cols[6].parse().unwrap_or(0.0);
            let top: f64 = cols[7].parse().unwrap_or(0.0);
            let w: f64 = cols[8].parse().unwrap_or(0.0);
            let h: f64 = cols[9].parse().unwrap_or(0.0);
            let conf: f32 = cols[10].parse().unwrap_or(-1.0);
            words.push(Word {
                text: text.to_string(),
                bbox: BBox {
                    x0: left * scale,
                    y0: top * scale,
                    x1: (left + w) * scale,
                    y1: (top + h) * scale,
                },
                confidence: (conf >= 0.0).then_some(conf),
            });
            if conf >= 0.0 {
                confidences.push(conf);
            }
        }
    }

    let ocr_confidence = if confidences.is_empty() {
        None
    } else {
        Some(confidences.iter().sum::<f32>() / confidences.len() as f32)
    };

    RawPage { page_no: 0, width, height, words, ocr_confidence }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_TSV: &str = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
1\t1\t0\t0\t0\t0\t0\t0\t2480\t3509\t-1\t\n\
2\t1\t1\t0\t0\t0\t138\t339\t631\t123\t-1\t\n\
5\t1\t1\t1\t1\t1\t138\t369\t213\t87\t89.73\tBryn\n\
5\t1\t1\t1\t1\t2\t376\t369\t318\t79\t81.82\tShander\n\
5\t1\t1\t1\t1\t3\t765\t339\t4\t6\t12.5\t.-\n";

    #[test]
    fn parses_words_and_mean_confidence_from_tsv() {
        let page = parse_tsv(SAMPLE_TSV, 300);
        assert_eq!(page.words.len(), 3);
        assert_eq!(page.words[0].text, "Bryn");
        assert_eq!(page.words[1].text, "Shander");
        assert!((page.width - 2480.0 * 72.0 / 300.0).abs() < 0.01);
        let conf = page.ocr_confidence.unwrap();
        assert!((conf - (89.73 + 81.82 + 12.5) / 3.0).abs() < 0.01);
    }

    #[test]
    fn empty_tsv_yields_no_confidence() {
        let page = parse_tsv("level\tpage_num\n1\t1\t0\t0\t0\t0\t0\t0\t100\t100\t-1\t\n", 300);
        assert!(page.words.is_empty());
        assert!(page.ocr_confidence.is_none());
    }
}
