//! Per-brain configuration, persisted as `.brain/config.toml` at the root
//! of a knowledge-base folder (a "brain") alongside its `raw/` and `wiki/`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How aggressively to run OCR during ingest.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrMode {
    /// OCR only pages whose native text layer looks unusably thin.
    #[default]
    Auto,
    /// Never OCR; thin-text pages are ingested with whatever (little)
    /// text poppler can find.
    Never,
    /// OCR every page, ignoring any native text layer.
    Always,
}

impl std::str::FromStr for OcrMode {
    type Err = crate::error::BrainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(OcrMode::Auto),
            "never" => Ok(OcrMode::Never),
            "always" => Ok(OcrMode::Always),
            other => Err(crate::error::BrainError::InvalidData(format!(
                "invalid ocr mode {other:?} (expected auto|never|always)"
            ))),
        }
    }
}

/// Persisted configuration for one brain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrainConfig {
    /// Name of the rule pack to use for indexing (matches a file under
    /// `packs/{name}.toml`, resolved relative to the brain or a shared
    /// packs directory).
    #[serde(default = "default_pack")]
    pub pack: String,
    /// Default OCR behavior for `brain ingest`.
    #[serde(default)]
    pub ocr: OcrMode,
    /// Minimum tesseract mean-word confidence (0-100) below which a page
    /// is flagged `low_confidence` and excluded from indexing.
    #[serde(default = "default_min_confidence")]
    pub min_ocr_confidence: f32,
    /// Character-count threshold below which a page's native text layer
    /// is considered too thin and a candidate for OCR fallback.
    #[serde(default = "default_thin_text_chars")]
    pub thin_text_chars: usize,
}

fn default_pack() -> String {
    "generic".to_string()
}

fn default_min_confidence() -> f32 {
    45.0
}

fn default_thin_text_chars() -> usize {
    100
}

impl Default for BrainConfig {
    fn default() -> Self {
        Self {
            pack: default_pack(),
            ocr: OcrMode::default(),
            min_ocr_confidence: default_min_confidence(),
            thin_text_chars: default_thin_text_chars(),
        }
    }
}

impl BrainConfig {
    /// The `.brain/` directory for a given brain root.
    pub fn brain_dir(root: &Path) -> PathBuf {
        root.join(".brain")
    }

    /// Path to this brain's config file.
    pub fn config_path(root: &Path) -> PathBuf {
        Self::brain_dir(root).join("config.toml")
    }

    /// Path to this brain's SQLite database.
    pub fn db_path(root: &Path) -> PathBuf {
        Self::brain_dir(root).join("graph.db")
    }

    /// Path to this brain's content-addressed extraction cache.
    pub fn cache_dir(root: &Path) -> PathBuf {
        Self::brain_dir(root).join("cache")
    }

    /// Loads config from `{root}/.brain/config.toml`.
    pub fn load(root: &Path) -> crate::error::Result<Self> {
        let path = Self::config_path(root);
        if !path.exists() {
            return Err(crate::error::BrainError::NotInitialized(
                root.display().to_string(),
            ));
        }
        let text = std::fs::read_to_string(&path)?;
        toml::from_str(&text).map_err(|e| {
            crate::error::BrainError::InvalidData(format!("{}: {e}", path.display()))
        })
    }

    /// Writes this config to `{root}/.brain/config.toml`, creating
    /// `.brain/` if needed.
    pub fn save(&self, root: &Path) -> crate::error::Result<()> {
        std::fs::create_dir_all(Self::brain_dir(root))?;
        let text = toml::to_string_pretty(self).map_err(|e| {
            crate::error::BrainError::InvalidData(format!("serializing config: {e}"))
        })?;
        std::fs::write(Self::config_path(root), text)?;
        Ok(())
    }
}
