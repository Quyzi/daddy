//! Shared domain types, IDs, config, and error handling for the `brain`
//! document-graph toolchain (`brain-extract`, `brain-layout`,
//! `brain-index`, `brain-store`, `brain-query`, `brain-wiki`, `brain-cli`).
//!
//! This crate defines vocabulary only — no I/O, no PDF parsing, no SQL.
//! Everything here is plain data plus small pure helpers (like [`slugify`]),
//! so it has effectively no failure modes of its own and is safe for every
//! downstream crate to depend on without dragging in heavyweight deps.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod error;
pub mod geometry;
pub mod ids;
pub mod model;

pub use config::{BrainConfig, OcrMode};
pub use error::{BrainError, Result};
pub use geometry::{BBox, RawPage, Word};
pub use ids::{BlockId, ChunkId, DocId, EntityId, PageId, SectionId};
pub use model::{
    Block, BlockKind, Chunk, ChunkKind, Document, Edge, EdgeKind, Entity, EntityKind,
    ExtractorKind, Page, Section, slugify,
};
