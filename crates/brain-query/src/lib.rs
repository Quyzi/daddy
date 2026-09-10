//! Retrieval over a brain's graph: `brain explore` (ranked, budgeted,
//! cited search) and `brain get` (a direct entity lookup).
//!
//! Pipeline (see [`explore::explore`]): [`brain_store::Store::search_chunks`]
//! (FTS5 BM25) and [`brain_store::Store::search_entities_by_name`] seed a
//! candidate entity set -> [`walk::expand`] grows it along graph edges up
//! to a hop limit -> [`rank::combine_score`] scores every candidate
//! (text relevance, name match, centrality, hop decay) -> results are
//! kept in score order until a token budget is spent. [`get::get_entity`]
//! skips all of that for the simpler "I know exactly which entity I
//! want" case.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod explore;
pub mod get;
pub mod rank;
pub mod render;
pub mod types;
pub mod walk;

pub use explore::explore;
pub use get::get_entity;
pub use rank::{combine_score, normalize_bm25, ScoreWeights};
pub use render::{render_entity_markdown, render_explore_markdown};
pub use types::{Citation, EntityView, ExploreOptions, ExploreResult, FieldValue, Neighbour, ResultItem};
pub use walk::expand;
