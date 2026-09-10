//! Small helpers shared by both PDF-aware backends (`poppler` for native
//! text, `tesseract` for OCR) so page-count lookup isn't implemented twice.

use brain_core::error::{BrainError, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Locates `pdfinfo` on `PATH`.
pub(crate) fn locate_pdfinfo() -> Result<PathBuf> {
    which::which("pdfinfo")
        .map_err(|_| BrainError::Extraction("pdfinfo not found on PATH (install poppler-utils)".into()))
}

/// Runs `pdfinfo` and returns the document's page count, if parseable.
pub(crate) fn page_count(pdfinfo_bin: &Path, path: &Path) -> Result<Option<u32>> {
    let output = Command::new(pdfinfo_bin)
        .arg(path)
        .output()
        .map_err(|e| BrainError::Extraction(format!("running pdfinfo: {e}")))?;
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .find_map(|l| l.strip_prefix("Pages:"))
        .and_then(|n| n.trim().parse::<u32>().ok()))
}

/// Expands a [`crate::PageRange`] into a concrete inclusive `(start, end)`
/// pair, querying `pdfinfo` for the total page count when the range is
/// [`crate::PageRange::All`].
pub(crate) fn resolve_range(
    pdfinfo_bin: &Path,
    path: &Path,
    pages: crate::PageRange,
) -> Result<(u32, u32)> {
    match pages {
        crate::PageRange::Range(start, end) => Ok((start, end)),
        crate::PageRange::All => {
            let total = page_count(pdfinfo_bin, path)?.ok_or_else(|| {
                BrainError::Extraction(format!("could not determine page count for {}", path.display()))
            })?;
            Ok((1, total))
        }
    }
}
