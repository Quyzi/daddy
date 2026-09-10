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
use crate::wikilink;
use brain_core::error::Result;
use brain_core::{slugify, Chunk, DocId, Edge, EdgeKind, Entity, EntityId, EntityKind};
use brain_store::Store;
use std::collections::{HashMap, HashSet};

/// Starting weight for a [`EdgeKind::LinksTo`] edge (an explicit,
/// human-authored `[[wikilink]]`), relative to the `1.0` every
/// auto-derived `Mentions`/`CoOccurs` edge starts at (see `edges.rs`). A
/// person deliberately linking two notes is stronger signal than one
/// incidental name mention, so it starts already worth two of those —
/// `Store::add_edge` still accumulates weight on repetition either way,
/// so enough incidental mentions can still catch up over a large corpus.
const LINKS_TO_WEIGHT: f64 = 2.0;

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

    // Which entity "represents" each document as a whole, for wikilink/
    // tag resolution below: the entity recognized from the *first*
    // section in reading order — for a Markdown/Obsidian note that's
    // always its own title heading, so the note itself becomes the thing
    // a `[[wikilink]]` to it resolves to. PDFs get an entry here too
    // (their first heading's entity, if any), harmlessly unused since
    // wikilink syntax never appears in a rulebook's actual prose.
    let mut doc_primary_entity: HashMap<DocId, EntityId> = HashMap::new();

    for doc in store.list_documents()? {
        let doc_id = doc.id.expect("documents read back from storage always have an id");
        let ordered_blocks = store.list_blocks_for_document(doc_id)?;
        let sections = build_sections(&ordered_blocks, &field_labels);
        report.documents += 1;

        // Parallel to `sections`, filled in as each is inserted, so a
        // later section's `parent_index` (always an *earlier* index —
        // see `BuiltSection::parent_index`'s docs) can be resolved to a
        // real, already-assigned `SectionId`.
        let mut section_ids: Vec<brain_core::SectionId> = Vec::with_capacity(sections.len());

        for (section_idx, section) in sections.iter().enumerate() {
            let parent_id = section.parent_index.map(|idx| section_ids[idx]);
            let section_id = store.insert_section(&brain_core::Section {
                id: None,
                doc_id,
                parent_id,
                level: section.level,
                title: section.title.clone(),
                slug: section.slug.clone(),
                start_page: section.start_page,
                end_page: section.end_page,
            })?;
            section_ids.push(section_id);
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
            if section_idx == 0 {
                if let Some(eid) = entity_id {
                    doc_primary_entity.insert(doc_id, eid);
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

    run_gazetteer_pass(store, &mut report)?;
    resolve_wikilinks_and_tags(store, &doc_primary_entity, &mut report)?;

    report.entities = store.list_entities(None)?.len();

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

/// Resolves `[[wikilink]]`s and `#tag`s into graph edges, and frontmatter
/// `aliases:`/`tags:` into alias rows and tag entities — run once every
/// document's primary entity is known (see `doc_primary_entity`'s docs at
/// its call site), same phase as the gazetteer pass this follows.
///
/// Two sources of these, handled separately because they're stated at
/// different granularity: frontmatter is document-level metadata (stated
/// once, in `documents.frontmatter`), while inline `[[links]]`/`#tags`
/// are scanned from chunk text (so they carry evidence — the specific
/// chunk that mentioned them — that frontmatter-derived edges don't
/// have).
fn resolve_wikilinks_and_tags(
    store: &Store,
    doc_primary_entity: &HashMap<DocId, EntityId>,
    report: &mut IndexReport,
) -> Result<()> {
    let documents = store.list_documents()?;
    let mut doc_by_slug: HashMap<String, DocId> = HashMap::new();
    for doc in &documents {
        if let Some(id) = doc.id {
            doc_by_slug.insert(slugify(&doc.title), id);
        }
    }

    for doc in &documents {
        let Some(doc_id) = doc.id else { continue };
        let Some(&entity_id) = doc_primary_entity.get(&doc_id) else { continue };
        let Some(fm_text) = &doc.frontmatter else { continue };
        let Ok(fm) = serde_json::from_str::<serde_json::Value>(fm_text) else { continue };

        for alias in wikilink::frontmatter_list(&fm, "aliases") {
            store.add_alias(entity_id, &alias, &wikilink::normalize_alias(&alias))?;
        }
        for tag in wikilink::frontmatter_list(&fm, "tags") {
            link_to_tag(store, report, entity_id, &tag, None)?;
        }
    }

    for chunk in store.list_all_chunks()? {
        let Some(&src_entity) = doc_primary_entity.get(&chunk.doc_id) else { continue };

        for link in wikilink::extract_wikilinks(&chunk.text) {
            let target_slug = slugify(&link.target);
            let resolved = doc_by_slug.get(&target_slug).and_then(|d| doc_primary_entity.get(d));
            match resolved {
                Some(&dst_entity) if dst_entity != src_entity => {
                    store.add_edge(&Edge {
                        src: src_entity,
                        dst: dst_entity,
                        kind: EdgeKind::LinksTo,
                        weight: LINKS_TO_WEIGHT,
                        evidence_chunk_id: chunk.id,
                    })?;
                    report.edges += 1;
                }
                // A self-link or a target that isn't (yet) an ingested
                // document is not an error here — `brain lint` re-derives
                // exactly this same resolution at read time to report
                // unresolved wikilinks, rather than this pass needing to
                // persist a list of them itself (see `brain-wiki`'s lint
                // module).
                _ => {}
            }
        }

        for tag in wikilink::extract_tags(&chunk.text) {
            link_to_tag(store, report, src_entity, &tag, chunk.id)?;
        }
    }
    Ok(())
}

/// Upserts a `#tag` as an [`EntityKind::Other`] entity and links `from`
/// to it via a [`EdgeKind::Mentions`] edge (skipped if `from` already
/// *is* that tag entity — a document cannot mention its own tag).
fn link_to_tag(
    store: &Store,
    report: &mut IndexReport,
    from: EntityId,
    tag: &str,
    evidence_chunk_id: Option<brain_core::ChunkId>,
) -> Result<()> {
    let slug = slugify(tag);
    if slug.is_empty() {
        return Ok(());
    }
    let tag_entity = store.upsert_entity(&Entity {
        id: None,
        kind: EntityKind::Other("tag".to_string()),
        name: format!("#{tag}"),
        slug,
        canonical_id: None,
        centrality: 0.0,
        confidence: 1.0,
    })?;
    if tag_entity != from {
        store.add_edge(&Edge {
            src: from,
            dst: tag_entity,
            kind: EdgeKind::Mentions,
            weight: 1.0,
            evidence_chunk_id,
        })?;
        report.edges += 1;
    }
    Ok(())
}
