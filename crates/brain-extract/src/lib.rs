//! Extraction backends: turn a source file into positioned words.
//!
//! This crate defines the [`Extractor`] trait and the backends
//! implementing it ([`PopplerExtractor`] for native PDF text,
//! [`PlainExtractor`] for already-textual sources; OCR support lands in a
//! later phase as [`tesseract::TesseractExtractor`]), plus a
//! content-addressed [`Cache`] so extraction — the slow part of ingest —
//! runs at most once per distinct file.
//!
//! Every backend implements the same trait, so `brain-index`/`brain-cli`
//! never need to know which one produced a given [`brain_core::RawPage`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod asciidoc;
pub mod cache;
pub mod frontmatter;
pub mod html;
pub mod markdown;
mod pdfutil;
pub mod plain;
pub mod poppler;
pub mod select;
pub mod structured;
pub mod tesseract;
pub mod types;
pub mod url;

pub use asciidoc::AsciidocExtractor;
pub use cache::{sha256_bytes, sha256_file, Cache};
pub use markdown::MarkdownExtractor;
pub use plain::PlainExtractor;
pub use poppler::PopplerExtractor;
pub use select::{extract_auto, is_low_confidence};
pub use structured::{StructuredBlock, StructuredDoc, StructuredExtractor};
pub use tesseract::TesseractExtractor;
pub use types::{Capability, Extractor, PageRange};
pub use url::UrlExtractor;
