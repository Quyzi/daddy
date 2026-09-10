//! Rule-pack-driven indexing.
//!
//! Turns a document's already-stored, already-laid-out blocks (see
//! `brain-layout` and `brain-store`) into sections, chunks, recognized
//! entities, and the graph of edges between them — no AI involved.
//! Everything here is deterministic: same blocks + same rule pack always
//! produce the same graph, which is what makes `brain index` safe to
//! re-run after editing a rule pack without re-extracting or re-laying-out
//! anything.
//!
//! The pipeline, tied together by [`index_all`]: [`sections::build_sections`]
//! groups a document's blocks into heading-anchored sections ->
//! [`chunk::recognize`] applies a [`pack::CompiledPack`]'s rules to decide
//! what (if anything) each section defines -> [`chunk::build_chunks`]
//! turns it into one atomic chunk (a recognized entity) or several prose
//! chunks (generic content) -> once every document is processed,
//! [`gazetteer::Gazetteer`] scans all chunk text in one whole-corpus pass
//! -> [`edges::derive_edges_for_chunk`] turns co-occurring mentions into
//! graph edges -> [`pagerank::pagerank`] scores the resulting graph.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod chunk;
pub mod edges;
pub mod gazetteer;
pub mod orchestrate;
pub mod pack;
pub mod pagerank;
pub mod sections;
pub mod wikilink;

pub use chunk::{build_chunks, display_name, recognize, BuiltChunk, Recognized};
pub use edges::derive_edges_for_chunk;
pub use gazetteer::{Gazetteer, Mention};
pub use orchestrate::{index_all, IndexReport};
pub use pack::{CompiledPack, RulePack};
pub use pagerank::pagerank;
pub use sections::{build_sections, BuiltSection};
pub use wikilink::{extract_tags, extract_wikilinks, frontmatter_list, normalize_alias, WikiLink};
