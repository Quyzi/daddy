//! The [`StructuredExtractor`] trait: a second extraction path for
//! sources that already declare their own structure (Markdown, AsciiDoc,
//! fetched HTML, database rows) rather than needing it geometrically
//! guessed.
//!
//! `brain-layout`'s word-rejoin/column-detection/heading-ensemble
//! machinery exists to compensate for PDFs having *no* explicit
//! structure at all — a PDF is just words with coordinates. Markdown's
//! `#`/`##`, AsciiDoc's `=`/`==`, and an HTML document's `<h1>`-`<h6>`
//! already know their own heading depth; running them through the same
//! heuristics `brain-layout` needs for PDFs would throw that away and
//! re-guess it, worse. A [`StructuredExtractor`] instead builds final
//! [`StructuredBlock`]s directly, skipping `brain_layout::reconstruct_document`
//! entirely — `brain-cli`'s `ingest` command tells the two paths apart by
//! which trait a backend implements (see `select_extractor` in
//! `brain-cli`'s `ingest.rs`).

use brain_core::error::Result;
use brain_core::{BBox, BlockKind};
use std::path::Path;

use crate::types::Capability;

/// One block of a [`StructuredDoc`], analogous to
/// `brain_layout::LaidOutBlock` but for sources that already know their
/// own heading depth. Left without a `page_id` (there is no real paged
/// geometry here) — `brain-cli`'s ingest command inserts one synthetic
/// page per structured document and assigns each block's `page_id` when
/// it writes these to storage, the same two-step pattern already used
/// for a PDF's `LaidOutBlock`s.
#[derive(Debug, Clone)]
pub struct StructuredBlock {
    /// Always `0` for these sources — none of them have real columns.
    /// Kept so the shape lines up 1:1 with `brain_core::Block`.
    pub col: u32,
    /// Position within the document's reading order.
    pub ord: u32,
    /// A synthetic, order-preserving bounding box (see
    /// `plain::text_to_page`'s doc comment for why this convention
    /// exists): callers that only care about reading order never need a
    /// *real* geometry, just one that sorts correctly.
    pub bbox: BBox,
    /// Structural role.
    pub kind: BlockKind,
    /// The block's text.
    pub text: String,
    /// Heading depth, when this block is a heading: `Some(1)` for a
    /// top-level heading, `Some(2)` for the next depth down, and so on.
    /// `None` for every non-heading block. See
    /// [`brain_core::Block::heading_level`]'s docs for how this flows
    /// into real nested sections downstream.
    pub heading_level: Option<u8>,
}

/// One document produced by a [`StructuredExtractor`]. Almost always
/// exactly one per input file — the exception is a `.urls` list file
/// (see `crate::url`), which fans out into several independent
/// documents, one per URL, from a single input path.
#[derive(Debug, Clone)]
pub struct StructuredDoc {
    /// Human-readable title: a front-matter `title:` override, an HTML
    /// `<title>`, a URL entry's `///` doc-comment, or the filename stem.
    pub title: String,
    /// The citation string to record as this document's `path` — the
    /// original file path for Markdown/AsciiDoc, but the *URL itself*
    /// for a fetched web page (not wherever its `.url`/`.urls` pointer
    /// file happens to live).
    pub source_path: String,
    /// JSON-serialized front matter/metadata, if the source declared
    /// any — stored verbatim in [`brain_core::Document::frontmatter`].
    pub frontmatter: Option<String>,
    /// The document's blocks, in final reading order.
    pub blocks: Vec<StructuredBlock>,
}

/// A backend for sources with explicit structure of their own, producing
/// final blocks directly instead of positioned words for `brain-layout`
/// to reconstruct. See the module docs for why this exists as a second
/// path alongside [`crate::types::Extractor`] rather than folding into it.
pub trait StructuredExtractor: Send + Sync {
    /// Cheap probe: can this backend handle `path`, and how well?
    fn probe(&self, path: &Path) -> Result<Capability>;

    /// Extracts every document `path` yields. A `.md`/`.adoc` file always
    /// yields exactly one; a `.urls` list yields one per URL entry.
    fn extract_structured(&self, path: &Path) -> Result<Vec<StructuredDoc>>;

    /// A short, stable name for logging and document bookkeeping.
    fn name(&self) -> &'static str;
}

/// Builds a synthetic, order-preserving bounding box for the `n`th block
/// on a synthetic single page — the same convention `plain::text_to_page`
/// uses for already-linear text: each block gets its own `y`, so reading
/// order survives a round trip through storage with no real geometry to
/// preserve. Shared by every [`StructuredExtractor`] backend so their
/// output composes identically once written.
pub fn synthetic_bbox(ord: u32) -> BBox {
    let y = ord as f64;
    BBox { x0: 0.0, y0: y, x1: 1.0, y1: y + 1.0 }
}
