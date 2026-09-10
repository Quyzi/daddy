//! Ties every stage in this crate together into the one function `brain
//! index` actually calls: rebuild sections/chunks/entities from a
//! brain's already-stored blocks, then run one whole-corpus gazetteer +
//! edge + centrality pass. See each submodule for why the individual
//! stages work the way they do.

use crate::chunk::{build_chunks, display_name, recognize};
use crate::gazetteer::Gazetteer;
use crate::edges::derive_edges_for_chunk;
use crate::pack::CompiledPack;
use crate::pagerank::pagerank;
use crate::sections::build_sections;
use brain_core::error::Result;
use brain_core::{slugify, Chunk, Entity, EntityId, EntityKind};
use brain_store::Store;
use std::collections::HashSet;

/// Entity names shorter than this are never registered as entities at
/// all. A heading-sized fragment this short is essentially always noise
/// — a lone table-of-contents leader character, an orphaned list bullet
/// — not a real concept worth a graph node; the gazetteer already
/// refuses to *match* names this short (see `gazetteer.rs`), so letting
/// them through as entities would only ever produce unmentionable,
/// unlinkable clutter (confirmed against a real multi-book index run
/// during development, which otherwise surfaced dozens of single- and
/// two-letter "entities").
const MIN_ENTITY_NAME_LEN: usize = 3;

/// Summary counts from one `index_all` run, printed by `brain index`.
#[derive(Debug, Default, Clone, Copy)]
pub struct IndexReport {
    /// Documents processed.
    pub documents: usize,
    /// Sections built.
    pub sections: usize,
    /// Chunks written.
    pub chunks: usize,
    /// Distinct entities recognized (after (kind, slug) dedup).
    pub entities: usize,
    /// Mention rows recorded by the gazetteer pass.
    pub mentions: usize,
    /// Edges written (after weight accumulation).
    pub edges: usize,
}

/// Rebuilds the entire derived index (sections, chunks, entities, edges,
/// centrality) from a brain's already-stored documents/pages/blocks.
/// Safe and cheap to re-run after editing a rule pack — nothing here
/// re-extracts or re-lays-out anything.
pub fn index_all(store: &mut Store, pack: &CompiledPack) -> Result<IndexReport> {
    store.clear_index()?;
    let mut report = IndexReport::default();

    // Wrap the whole rebuild in one transaction. Left on autocommit,
    // SQLite fsyncs on every single INSERT -- fine for a handful of
    // rows, but a multi-book corpus produces tens of thousands of
    // sections/chunks/mentions/edges, and one fsync each turns a
    // sub-minute rebuild into a many-minutes one (measured directly
    // during development on a 3-book smoke test).
    store
        .conn()
        .execute_batch("BEGIN")
        .map_err(|e| brain_core::error::BrainError::Db(e.to_string()))?;

    // Track which entity already has a primary definition, so the first
    // document to define something (in document-list order) wins that
    // distinction rather than every re-definition claiming it.
    let mut has_primary: HashSet<brain_core::EntityId> = HashSet::new();
    let field_labels = pack.all_field_labels();

    for doc in store.list_documents()? {
        let doc_id = doc.id.expect("documents read back from storage always have an id");
        let ordered_blocks = store.list_blocks_for_document(doc_id)?;
        let sections = build_sections(&ordered_blocks, &field_labels);
        report.documents += 1;

        for section in &sections {
            let section_id = store.insert_section(&brain_core::Section {
                id: None,
                doc_id,
                parent_id: None,
                level: 1,
                title: section.title.clone(),
                slug: section.slug.clone(),
                start_page: section.start_page,
                end_page: section.end_page,
            })?;
            report.sections += 1;

            let recognized = recognize(section, pack);
            let built_chunks = build_chunks(section, recognized.as_ref());

            let mut entity_id: Option<EntityId> = None;
            if let Some(r) = &recognized {
                let name = display_name(&section.title);
                let slug = slugify(&name);
                if name.chars().count() >= MIN_ENTITY_NAME_LEN && !slug.is_empty() {
                    let kind = EntityKind::parse(&r.kind);
                    entity_id = Some(store.upsert_entity(&Entity {
                        id: None,
                        kind,
                        name,
                        slug,
                        canonical_id: None,
                        centrality: 0.0,
                        confidence: 1.0,
                    })?);
                }
            }

            for (i, built) in built_chunks.iter().enumerate() {
                let chunk_id = store.insert_chunk(&Chunk {
                    id: None,
                    doc_id,
                    section_id: Some(section_id),
                    start_page: section.start_page,
                    end_page: section.end_page,
                    ord: i as u32,
                    kind: built.kind,
                    text: built.text.clone(),
                    token_est: (built.text.len() / 4) as u32,
                })?;
                report.chunks += 1;

                if i == 0 {
                    if let Some(eid) = entity_id {
                        let is_primary = has_primary.insert(eid);
                        store.add_definition(eid, chunk_id, doc_id, section.start_page, is_primary)?;
                        if let Some(r) = &recognized {
                            for (key, value) in &r.fields {
                                store.set_field(eid, doc_id, key, value)?;
                            }
                        }
                    }
                }
            }
        }
    }

    report.entities = store.list_entities(None)?.len();

    run_gazetteer_pass(store, &mut report)?;

    let entity_ids: Vec<_> = store.list_entities(None)?.iter().filter_map(|e| e.id).collect();
    let all_edges = store.all_edges()?;
    let ranks = pagerank(&entity_ids, &all_edges);
    for (id, rank) in ranks {
        store.set_entity_centrality(id, rank)?;
    }

    store
        .conn()
        .execute_batch("COMMIT")
        .map_err(|e| brain_core::error::BrainError::Db(e.to_string()))?;
    Ok(report)
}

/// Whole-corpus mention scanning + edge derivation, run once after every
/// document's entities are known (see `gazetteer.rs`'s module docs for
/// why this has to be a single global pass rather than per-document).
fn run_gazetteer_pass(store: &Store, report: &mut IndexReport) -> Result<()> {
    let entities = store.list_entities(None)?;
    let entries: Vec<(brain_core::EntityId, String)> =
        entities.iter().filter_map(|e| e.id.map(|id| (id, e.name.clone()))).collect();
    let gazetteer = Gazetteer::build(&entries)?;

    for chunk in store.list_all_chunks()? {
        let chunk_id = match chunk.id {
            Some(id) => id,
            None => continue,
        };
        let mentions = gazetteer.scan(&chunk.text);
        if mentions.is_empty() {
            continue;
        }
        for m in &mentions {
            store.add_mention(chunk_id, m.entity_id, m.start, m.end, 1.0)?;
            report.mentions += 1;
        }

        let mentioned_ids: Vec<_> = mentions.iter().map(|m| m.entity_id).collect();
        let defining = store.defining_entity_for_chunk(chunk_id)?;
        for edge in derive_edges_for_chunk(&mentioned_ids, defining) {
            store.add_edge(&edge)?;
            report.edges += 1;
        }
    }
    Ok(())
}
