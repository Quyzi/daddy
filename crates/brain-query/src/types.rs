//! Output types shared by `brain explore` and `brain get`.

use brain_core::{ChunkKind, EntityId};
use serde::Serialize;

/// Tunable knobs for one `explore` call.
#[derive(Debug, Clone)]
pub struct ExploreOptions {
    /// Approximate token budget for the returned text (chars / 4).
    pub budget_tokens: usize,
    /// Number of graph hops to expand from seed entity matches.
    pub hops: u32,
    /// Restrict results to one entity kind (e.g. `"spell"`), if given.
    pub kind: Option<String>,
    /// Maximum number of result items to consider before budgeting.
    pub candidate_limit: usize,
}

impl Default for ExploreOptions {
    fn default() -> Self {
        Self { budget_tokens: 8000, hops: 2, kind: None, candidate_limit: 100 }
    }
}

/// Where one piece of retrieved text came from, for citation.
#[derive(Debug, Clone, Serialize)]
pub struct Citation {
    /// Source document title.
    pub document: String,
    /// Page the text starts on.
    pub page: u32,
}

/// One ranked, citable piece of retrieved text.
#[derive(Debug, Clone, Serialize)]
pub struct ResultItem {
    /// The entity this chunk defines, if any.
    pub entity_name: Option<String>,
    /// That entity's kind, if any.
    pub entity_kind: Option<String>,
    /// The chunk's kind (definition vs. prose).
    pub chunk_kind: ChunkKind,
    /// The chunk's full text.
    pub text: String,
    /// Where it came from.
    pub citation: Citation,
    /// Final combined relevance score (higher is better); exposed mainly
    /// for debugging/tests, not a stable public contract.
    pub score: f64,
}

/// The full result of one `explore` call.
#[derive(Debug, Clone, Serialize)]
pub struct ExploreResult {
    /// The query as given.
    pub query: String,
    /// Ranked, budgeted result items.
    pub items: Vec<ResultItem>,
    /// Sum of `items`' estimated token counts.
    pub total_tokens: usize,
}

/// One field value for an entity, with its source (for `brain get`,
/// which shows every document's stated value rather than picking one —
/// see [`crate::get::get_entity`]'s docs on why).
#[derive(Debug, Clone, Serialize)]
pub struct FieldValue {
    /// Field name.
    pub key: String,
    /// Value as stated in `document`.
    pub value: String,
    /// Which document stated it.
    pub document: String,
}

/// A graph neighbour of an entity, for `brain get`'s "related" section.
#[derive(Debug, Clone, Serialize)]
pub struct Neighbour {
    /// The neighbouring entity's id.
    #[serde(skip)]
    pub id: EntityId,
    /// Its name.
    pub name: String,
    /// Its kind.
    pub kind: String,
    /// The edge's weight (strength of the relationship).
    pub weight: f64,
}

/// Everything `brain get <entity>` prints: a verbatim definition, its
/// fields, and its graph neighbourhood.
#[derive(Debug, Clone, Serialize)]
pub struct EntityView {
    /// Canonical display name.
    pub name: String,
    /// Entity kind.
    pub kind: String,
    /// Verbatim primary definition text, if the entity has one.
    pub definition: Option<String>,
    /// Where the definition came from.
    pub citation: Option<Citation>,
    /// Structured field values, across every document that states one.
    pub fields: Vec<FieldValue>,
    /// Graph neighbours, heaviest edge first.
    pub neighbours: Vec<Neighbour>,
}
