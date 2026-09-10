//! Page geometry: bounding boxes and positioned words.
//!
//! This is the common currency between `brain-extract` (which produces it
//! from PDFs/OCR) and `brain-layout` (which consumes it to reconstruct
//! reading order). Units are PDF points (1/72 inch) with the origin at the
//! top-left of the page, matching `pdftotext -bbox-layout` conventions.

use serde::{Deserialize, Serialize};

/// An axis-aligned bounding box in page-point coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    /// Left edge.
    pub x0: f64,
    /// Top edge.
    pub y0: f64,
    /// Right edge.
    pub x1: f64,
    /// Bottom edge.
    pub y1: f64,
}

impl BBox {
    /// Width of the box.
    pub fn width(&self) -> f64 {
        (self.x1 - self.x0).max(0.0)
    }

    /// Height of the box.
    pub fn height(&self) -> f64 {
        (self.y1 - self.y0).max(0.0)
    }

    /// Vertical center.
    pub fn y_center(&self) -> f64 {
        (self.y0 + self.y1) / 2.0
    }

    /// Returns the smallest box containing both `self` and `other`.
    pub fn union(&self, other: &BBox) -> BBox {
        BBox {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// Fraction of `self`'s height that overlaps `other` vertically, in `[0, 1]`.
    /// Used to decide whether two words sit on the same visual line.
    pub fn vertical_overlap_ratio(&self, other: &BBox) -> f64 {
        let top = self.y0.max(other.y0);
        let bottom = self.y1.min(other.y1);
        let overlap = (bottom - top).max(0.0);
        let shortest = self.height().min(other.height());
        if shortest <= 0.0 {
            0.0
        } else {
            overlap / shortest
        }
    }
}

/// One word (whitespace-delimited token) at a known position on a page,
/// as produced by an [`Extractor`](../brain_extract/trait.Extractor.html).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Word {
    /// The word's text, exactly as extracted (may be a sub-word fragment
    /// from OCR word-splitting; `brain-layout` rejoins these).
    pub text: String,
    /// Position on the page.
    pub bbox: BBox,
    /// OCR confidence in `[0, 100]` if this word came from OCR, `None` for
    /// native PDF text (which has no comparable confidence signal).
    pub confidence: Option<f32>,
}

/// A page's raw, unordered extraction output: just words with positions.
/// Everything about column order, headings, and structure is layered on
/// top by `brain-layout` — this type carries no interpretation at all.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawPage {
    /// 1-based page number within the source document.
    pub page_no: u32,
    /// Page width in points.
    pub width: f64,
    /// Page height in points.
    pub height: f64,
    /// Every word detected on the page, in extractor-native order
    /// (not necessarily reading order).
    pub words: Vec<Word>,
    /// Mean OCR confidence for the page, if it went through OCR.
    pub ocr_confidence: Option<f32>,
}

impl RawPage {
    /// Total extracted character count, used as a cheap "did this page
    /// have a usable text layer" signal before falling back to OCR.
    pub fn char_count(&self) -> usize {
        self.words.iter().map(|w| w.text.chars().count()).sum()
    }
}
