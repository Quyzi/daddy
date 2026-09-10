//! Content-addressed cache for extraction output.
//!
//! Extraction (and especially OCR) is the slow part of ingest — this cache
//! is what makes rule-pack iteration ("change a regex, re-run `brain
//! index`") cheap: as long as a source file's bytes haven't changed, its
//! extracted [`RawPage`]s are read back from disk instead of re-extracted.

use brain_core::error::{BrainError, Result};
use brain_core::RawPage;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Computes the SHA-256 of a file's contents, streamed in 1 MiB chunks so
/// hashing a multi-hundred-megabyte PDF doesn't require loading it whole.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Computes the SHA-256 of in-memory bytes — the dedup key for a
/// structured document (Markdown/AsciiDoc/a fetched URL/a datasource
/// query result), whose content isn't simply "the bytes of one file on
/// disk" the way a PDF's or plain-text file's is (a `.urls` list's own
/// file bytes don't change when a URL's live content does; see
/// `brain-cli`'s `ingest.rs` for how this is used).
pub fn sha256_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// A directory of `{sha256}/pages.json.zst` blobs, one per distinct source
/// file ever ingested.
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    /// Opens (creating if needed) a cache rooted at `dir`, typically
    /// `{brain}/.brain/cache`.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn blob_path(&self, sha256: &str) -> PathBuf {
        self.dir.join(sha256).join("pages.json.zst")
    }

    /// Returns whether cached pages already exist for this content hash.
    pub fn has(&self, sha256: &str) -> bool {
        self.blob_path(sha256).exists()
    }

    /// Reads back previously cached pages, if any.
    pub fn get(&self, sha256: &str) -> Result<Option<Vec<RawPage>>> {
        let path = self.blob_path(sha256);
        if !path.exists() {
            return Ok(None);
        }
        let compressed = std::fs::read(&path)?;
        let json = zstd::stream::decode_all(compressed.as_slice())
            .map_err(|e| BrainError::Extraction(format!("decompressing cache entry: {e}")))?;
        let pages = serde_json::from_slice(&json)
            .map_err(|e| BrainError::Extraction(format!("deserializing cache entry: {e}")))?;
        Ok(Some(pages))
    }

    /// Writes pages to the cache under `sha256`, replacing any existing
    /// entry. Writes to a temp file first and renames into place so a
    /// crash mid-write never leaves a corrupt blob for `get` to trip over.
    pub fn put(&self, sha256: &str, pages: &[RawPage]) -> Result<()> {
        let path = self.blob_path(sha256);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec(pages)
            .map_err(|e| BrainError::Extraction(format!("serializing cache entry: {e}")))?;
        let compressed = zstd::stream::encode_all(json.as_slice(), 3)
            .map_err(|e| BrainError::Extraction(format!("compressing cache entry: {e}")))?;
        let tmp = path.with_extension("zst.tmp");
        std::fs::write(&tmp, &compressed)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::{BBox, Word};
    use tempfile::tempdir;

    #[test]
    fn hashing_is_stable_and_content_sensitive() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, b"hello").unwrap();
        std::fs::write(&b, b"hello").unwrap();
        let ha = sha256_file(&a).unwrap();
        let hb = sha256_file(&b).unwrap();
        assert_eq!(ha, hb, "identical content must hash identically");

        std::fs::write(&b, b"goodbye").unwrap();
        let hb2 = sha256_file(&b).unwrap();
        assert_ne!(ha, hb2);
    }

    #[test]
    fn put_then_get_roundtrips_pages() {
        let dir = tempdir().unwrap();
        let cache = Cache::new(dir.path().join("cache")).unwrap();
        let pages = vec![RawPage {
            page_no: 1,
            width: 612.0,
            height: 792.0,
            words: vec![Word {
                text: "hello".into(),
                bbox: BBox { x0: 0.0, y0: 0.0, x1: 10.0, y1: 10.0 },
                confidence: None,
            }],
            ocr_confidence: None,
        }];
        assert!(!cache.has("deadbeef"));
        cache.put("deadbeef", &pages).unwrap();
        assert!(cache.has("deadbeef"));
        let back = cache.get("deadbeef").unwrap().unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].words[0].text, "hello");
    }

    #[test]
    fn missing_entry_returns_none_not_error() {
        let dir = tempdir().unwrap();
        let cache = Cache::new(dir.path().join("cache")).unwrap();
        assert!(cache.get("nonexistent").unwrap().is_none());
    }
}
