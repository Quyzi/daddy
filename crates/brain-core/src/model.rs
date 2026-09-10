//! Domain model shared across the pipeline: documents, blocks, sections,
//! chunks, entities, and the edges that connect them into a graph.

use crate::ids::{BlockId, ChunkId, DocId, EntityId, PageId, SectionId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The extractor backend that produced a document's text, recorded for
/// diagnostics (e.g. "why does this page look garbled" -> check `ocr`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtractorKind {
    /// Native PDF text layer via poppler.
    Poppler,
    /// Rendered + OCR'd via tesseract.
    Tesseract,
    /// Plain text/markdown/html/json read directly.
    Plain,
}

impl ExtractorKind {
    /// Stable string form stored in SQLite.
    pub fn as_str(&self) -> &'static str {
        match self {
            ExtractorKind::Poppler => "poppler",
            ExtractorKind::Tesseract => "tesseract",
            ExtractorKind::Plain => "plain",
        }
    }
}

impl std::str::FromStr for ExtractorKind {
    type Err = crate::error::BrainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "poppler" => Ok(ExtractorKind::Poppler),
            "tesseract" => Ok(ExtractorKind::Tesseract),
            "plain" => Ok(ExtractorKind::Plain),
            other => Err(crate::error::BrainError::InvalidData(format!(
                "unknown extractor kind {other:?}"
            ))),
        }
    }
}

/// One page of a document after layout reconstruction: plain reading-order
/// text plus enough metadata to judge its quality.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    /// Row id, assigned on insert.
    pub id: Option<PageId>,
    /// Owning document.
    pub doc_id: DocId,
    /// 1-based page number.
    pub page_no: u32,
    /// Page width in points.
    pub width: f64,
    /// Page height in points.
    pub height: f64,
    /// Final reading-order text (chrome stripped, words rejoined).
    pub text: String,
    /// Mean OCR confidence for the page, if OCR'd.
    pub ocr_conf: Option<f32>,
    /// Set when the page's text is unreliable (thin native layer with no
    /// OCR fallback, or OCR confidence below the configured threshold) and
    /// should be excluded from indexing.
    pub low_confidence: bool,
}

/// A single ingested source document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Row id, assigned on insert.
    pub id: Option<DocId>,
    /// Absolute path to the source file at ingest time.
    pub path: String,
    /// SHA-256 of the file's bytes; the cache and dedup key.
    pub sha256: String,
    /// Best-effort human title (filename stem, cleaned up).
    pub title: String,
    /// File extension / source kind ("pdf", "md", "txt", ...).
    pub kind: String,
    /// Total page count (1 for non-paginated text sources).
    pub page_count: u32,
    /// File size in bytes.
    pub bytes: u64,
    /// When this document was ingested.
    pub ingested_at: DateTime<Utc>,
    /// Which extractor produced most of this document's text.
    pub extractor: ExtractorKind,
    /// Whether any page in this document required OCR.
    pub ocr: bool,
}

/// The structural role a layout block plays on a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    /// A detected heading/title line.
    Heading,
    /// Ordinary paragraph text.
    Body,
    /// A tabular layout (columns of aligned numbers/short cells).
    Table,
    /// A recognized game stat block (monster or similar structured entry).
    StatBlock,
    /// Image/figure caption text.
    Caption,
    /// Running header/footer chrome (page numbers, book title) — excluded
    /// from indexing.
    Chrome,
}

/// One block of laid-out text on a page, in final reading order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// Row id, assigned on insert.
    pub id: Option<BlockId>,
    /// Owning page.
    pub page_id: PageId,
    /// 0-based column index on the page (0 = leftmost).
    pub col: u32,
    /// Position of this block within the page's reading order.
    pub ord: u32,
    /// Bounding box, in page points.
    pub bbox: crate::geometry::BBox,
    /// Structural role.
    pub kind: BlockKind,
    /// The block's rejoined, whitespace-normalized text.
    pub text: String,
}

/// A heading-derived section of a document, forming a tree via `parent_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Section {
    /// Row id, assigned on insert.
    pub id: Option<SectionId>,
    /// Owning document.
    pub doc_id: DocId,
    /// Parent section, if nested (`None` for top-level sections).
    pub parent_id: Option<SectionId>,
    /// Heading depth (1 = top-level chapter, 2 = subsection, ...).
    pub level: u32,
    /// The section's heading text.
    pub title: String,
    /// URL/filename-safe slug derived from `title`.
    pub slug: String,
    /// First page of the section.
    pub start_page: u32,
    /// Last page of the section (inclusive).
    pub end_page: u32,
}

/// The kind of content a chunk holds, used to weight retrieval and to
/// decide whether it may be split further.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkKind {
    /// Ordinary prose, safe to split at paragraph boundaries.
    Prose,
    /// A structured entity definition (spell/monster/item/...) — atomic,
    /// never split.
    Definition,
}

/// One indexable passage: the unit that FTS5 and the query engine operate
/// over.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    /// Row id, assigned on insert.
    pub id: Option<ChunkId>,
    /// Owning document.
    pub doc_id: DocId,
    /// Owning section, if the document has section structure.
    pub section_id: Option<SectionId>,
    /// First page the chunk's text appears on.
    pub start_page: u32,
    /// Last page the chunk's text appears on (inclusive).
    pub end_page: u32,
    /// Position of this chunk within its document.
    pub ord: u32,
    /// What kind of content this is.
    pub kind: ChunkKind,
    /// The chunk's full text.
    pub text: String,
    /// Rough token estimate (chars / 4), used for query budgeting.
    pub token_est: u32,
}

/// The kind of real-world thing a graph entity represents. Rule packs may
/// introduce additional string kinds beyond this open-ended set; this
/// enum covers the built-in generic and D&D 5e kinds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    /// A generic heading-derived topic (works for any document).
    Topic,
    /// A D&D 5e spell.
    Spell,
    /// A D&D 5e monster/creature stat block.
    Monster,
    /// A magic item or piece of equipment.
    Item,
    /// A player class.
    Class,
    /// A player subclass.
    Subclass,
    /// A player race/lineage.
    Race,
    /// A named person/NPC.
    Person,
    /// A named place.
    Location,
    /// A rule pack's custom entity kind not covered above.
    Other(String),
}

impl EntityKind {
    /// Stable string form stored in SQLite.
    pub fn as_str(&self) -> String {
        match self {
            EntityKind::Topic => "topic".to_string(),
            EntityKind::Spell => "spell".to_string(),
            EntityKind::Monster => "monster".to_string(),
            EntityKind::Item => "item".to_string(),
            EntityKind::Class => "class".to_string(),
            EntityKind::Subclass => "subclass".to_string(),
            EntityKind::Race => "race".to_string(),
            EntityKind::Person => "person".to_string(),
            EntityKind::Location => "location".to_string(),
            EntityKind::Other(s) => s.clone(),
        }
    }

    /// Parses a stable string form back into an [`EntityKind`].
    pub fn parse(s: &str) -> Self {
        match s {
            "topic" => EntityKind::Topic,
            "spell" => EntityKind::Spell,
            "monster" => EntityKind::Monster,
            "item" => EntityKind::Item,
            "class" => EntityKind::Class,
            "subclass" => EntityKind::Subclass,
            "race" => EntityKind::Race,
            "person" => EntityKind::Person,
            "location" => EntityKind::Location,
            other => EntityKind::Other(other.to_string()),
        }
    }
}

/// A named thing the index has recognized: a spell, monster, class,
/// heading-derived topic, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Row id, assigned on insert.
    pub id: Option<EntityId>,
    /// The entity's kind.
    pub kind: EntityKind,
    /// Canonical display name.
    pub name: String,
    /// URL/filename-safe slug derived from `name`.
    pub slug: String,
    /// If this entity was merged into another (cross-book `same_as`
    /// resolution), the id of the canonical entity. `None` means this
    /// entity is itself canonical.
    pub canonical_id: Option<EntityId>,
    /// Weighted PageRank centrality within the entity graph, used as a
    /// ranking prior. Populated after indexing; `0.0` until then.
    pub centrality: f64,
    /// Recognizer confidence in `[0, 1]` for how sure the rule pack was.
    pub confidence: f32,
}

/// The kind of relationship an [`Edge`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// `src` is the primary definition site of `dst`.
    Defines,
    /// `src`'s text mentions `dst` by name.
    Mentions,
    /// `src` is structurally nested under `dst` (e.g. subclass under class).
    PartOf,
    /// `src`'s definition text references `dst` by name.
    References,
    /// `src` and `dst` co-occur in the same chunk.
    CoOccurs,
    /// `src` and `dst` are the same real-world entity across documents.
    SameAs,
    /// `src` and `dst` give conflicting field values for the same entity.
    Contradicts,
}

impl EdgeKind {
    /// Stable string form stored in SQLite.
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeKind::Defines => "defines",
            EdgeKind::Mentions => "mentions",
            EdgeKind::PartOf => "part_of",
            EdgeKind::References => "references",
            EdgeKind::CoOccurs => "co_occurs",
            EdgeKind::SameAs => "same_as",
            EdgeKind::Contradicts => "contradicts",
        }
    }
}

impl std::str::FromStr for EdgeKind {
    type Err = crate::error::BrainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "defines" => Ok(EdgeKind::Defines),
            "mentions" => Ok(EdgeKind::Mentions),
            "part_of" => Ok(EdgeKind::PartOf),
            "references" => Ok(EdgeKind::References),
            "co_occurs" => Ok(EdgeKind::CoOccurs),
            "same_as" => Ok(EdgeKind::SameAs),
            "contradicts" => Ok(EdgeKind::Contradicts),
            other => Err(crate::error::BrainError::InvalidData(format!(
                "unknown edge kind {other:?}"
            ))),
        }
    }
}

/// A directed, weighted relationship between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    /// Source entity.
    pub src: EntityId,
    /// Destination entity.
    pub dst: EntityId,
    /// Relationship kind.
    pub kind: EdgeKind,
    /// Relationship strength/frequency, used in ranking and PageRank.
    pub weight: f64,
    /// The chunk whose text justifies this edge, if any.
    pub evidence_chunk_id: Option<ChunkId>,
}

/// Slugifies free text into a lowercase, hyphenated, filesystem/URL-safe
/// identifier (e.g. `"Mirror Image"` -> `"mirror-image"`).
/// Slugs longer than this get truncated with a hash suffix (see
/// [`slugify`]'s docs) — comfortably under every common filesystem's
/// filename length limit (255 bytes on ext4/APFS/NTFS) even after
/// `brain-wiki` appends `.md`, while still leaving a slug long enough to
/// stay readable.
const MAX_SLUG_LEN: usize = 80;

/// Slugifies free text into a lowercase, hyphenated, filesystem/URL-safe
/// identifier (e.g. `"Mirror Image"` -> `"mirror-image"`).
///
/// Headings this project extracts from real PDFs are not always
/// well-behaved: a misdetected layout can glue an entire sentence (or a
/// mangled multi-word table row) into one "heading", producing a slug far
/// too long for a filesystem's filename limit — `brain-wiki` writing one
/// out as `wiki/{slug}.md` crashed with "File name too long" on exactly
/// such input during development. Truncating at `MAX_SLUG_LEN` characters and
/// appending a short hash of the *full* original text keeps the result
/// filesystem-safe while still disambiguating two different long
/// headings that happen to share a truncated prefix.
pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(MAX_SLUG_LEN));
    let mut prev_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }

    if out.len() > MAX_SLUG_LEN {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        s.hash(&mut hasher);
        let mut end = MAX_SLUG_LEN;
        while end > 0 && !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
        out = out.trim_end_matches('-').to_string();
        out.push_str(&format!("-{:x}", hasher.finish() & 0xFFFF_FFFF));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Mirror Image"), "mirror-image");
        assert_eq!(slugify("Storm King's Thunder"), "storm-king-s-thunder");
        assert_eq!(slugify("  leading/trailing  "), "leading-trailing");
        assert_eq!(slugify("d20"), "d20");
    }

    #[test]
    fn slugify_truncates_pathologically_long_headings_with_a_disambiguating_suffix() {
        // Mirrors a real garbled multi-word heading a layout misdetection
        // produced during development, long enough to blow past a
        // filesystem's filename limit once `brain-wiki` appends `.md`.
        let long_heading = "Self made Pack The shukankor has advantage on attack rolls \
            against a creature if at least one of its duplicates is within five feet";
        let slug = slugify(long_heading);
        assert!(slug.len() <= MAX_SLUG_LEN + 12, "must stay well under filesystem filename limits: {slug:?}");

        // Two different long headings sharing the same truncated prefix
        // must not collide.
        let other_long_heading = format!("{long_heading} but with a different ending entirely");
        let other_slug = slugify(&other_long_heading);
        assert_ne!(slug, other_slug, "distinct long headings must not collapse to the same slug");
    }

    #[test]
    fn slugify_is_unaffected_for_ordinary_short_input() {
        assert_eq!(slugify("Fireball"), "fireball");
    }
}
