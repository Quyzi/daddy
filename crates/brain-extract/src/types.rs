//! The [`Extractor`] trait and the small types around it.

use brain_core::{RawPage, Result};
use std::path::Path;

/// A 1-based, inclusive page range to extract. `All` means the whole
/// document — callers extracting a single page still go through this so
/// large documents can be pulled incrementally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageRange {
    /// Every page in the document.
    All,
    /// Pages `start..=end`, both 1-based and inclusive.
    Range(u32, u32),
}

impl PageRange {
    /// A range covering a single page.
    pub fn single(page: u32) -> Self {
        PageRange::Range(page, page)
    }
}

/// What a backend reports after a cheap look at a file, before doing the
/// (potentially expensive) full extraction.
#[derive(Debug, Clone, Default)]
pub struct Capability {
    /// Whether this backend can handle the file at all.
    pub supported: bool,
    /// Total page count, if cheaply knowable (e.g. via `pdfinfo`).
    pub page_count: Option<u32>,
    /// Free-form diagnostic (e.g. why `supported` is false).
    pub note: Option<String>,
}

/// A backend that turns one source file into positioned words.
///
/// Implementations do no interpretation beyond "where is each word on the
/// page" — reading order, headings, and structure are entirely
/// `brain-layout`'s job. This keeps every backend swappable: a future
/// pure-Rust PDF parser can implement this trait without touching anything
/// downstream.
pub trait Extractor: Send + Sync {
    /// Cheap probe: can this backend handle `path`, and how well?
    fn probe(&self, path: &Path) -> Result<Capability>;

    /// Extracts `pages` from `path` as positioned words, one [`RawPage`]
    /// per page actually found in that range.
    fn extract(&self, path: &Path, pages: PageRange) -> Result<Vec<RawPage>>;

    /// A short, stable name for logging and cache/document bookkeeping.
    fn name(&self) -> &'static str;
}
