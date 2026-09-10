//! The workspace-wide error type. Every crate's fallible functions return
//! `Result<T, BrainError>` (or wrap it via `#[from]`), so callers never have
//! to juggle N different per-crate error enums.

use thiserror::Error;

/// Errors produced anywhere in the `brain` toolchain.
#[derive(Debug, Error)]
pub enum BrainError {
    /// An I/O operation failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A SQLite operation failed.
    #[error("database error: {0}")]
    Db(String),

    /// An external tool (poppler, tesseract) was missing, failed, or
    /// returned output the parser didn't understand.
    #[error("extraction error: {0}")]
    Extraction(String),

    /// Data read back from storage or a rule pack didn't match the
    /// expected shape.
    #[error("invalid data: {0}")]
    InvalidData(String),

    /// A rule pack (TOML) was malformed.
    #[error("rule pack error: {0}")]
    RulePack(String),

    /// The requested entity, document, or page was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// The brain directory (`.brain/`) is missing or not initialized.
    #[error("brain not initialized at {0} (run `brain init` first)")]
    NotInitialized(String),
}

/// Convenience alias used throughout the workspace.
pub type Result<T> = std::result::Result<T, BrainError>;
