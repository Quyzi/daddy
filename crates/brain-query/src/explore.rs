//! Top-level `brain explore` query: FTS5 text search + entity name
//! matching seed a graph walk, every candidate is scored, and the
//! highest-scoring results are returned up to a token budget.

use crate::rank::{combine_score, normalize_bm25, ScoreWeights};
use crate::types::{Citation, ExploreOptions, ExploreResult, ResultItem};
use crate::walk::expand;
use brain_core::error::Result;
use brain_core::{ChunkId, EntityId};
use brain_store::Store;
use std::collections::{HashMap, HashSet};

/// Turns a free-text query into an FTS5 `MATCH` expression: each word is
/// quoted (neutralizing FTS5 syntax characters like `:`/`-`/`*` a raw
/// query might contain) and OR'd together, favoring recall over
/// precision — ranking, not the query syntax, is what surfaces the best
/// matches first.
fn sanitize_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|w| format!("\"{}\"", w.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Runs one `explore` query against `store`. See the module-level docs
/// and the implementation plan's Phase 5 design for the pipeline this
/// implements: FTS5 + name-match seeds -> graph-walk expansion -> scored
/// ranking -> token-budgeted output.
pub fn explore(store: &Store, query: &str, opts: &ExploreOptions) -> Result<ExploreResult> {
    let weights = ScoreWeights::default();

    let fts_query = sanitize_fts_query(query);
    let fts_hits = if fts_query.is_empty() { Vec::new() } else { store.search_chunks(&fts_query, opts.candidate_limit)? };

    let name_hits = store.search_entities_by_name(query, 20)?;
    let name_match_ids: HashSet<EntityId> = name_hits.iter().filter_map(|e| e.id).collect();

    let mut seed_ids: HashSet<EntityId> = name_match_ids.clone();
    for hit in &fts_hits {
        if let Some(eid) = store.defining_entity_for_chunk(hit.chunk_id)? {
            seed_ids.insert(eid);
        }
    }

    let seeds: Vec<EntityId> = seed_ids.iter().copied().collect();
    let hop_distances = if opts.hops > 0 { expand(store, &seeds, opts.hops)? } else { HashMap::new() };

    // Every candidate entity, mapped to its hop distance from a seed
    // (`None` for the seeds themselves, i.e. undecayed).
    let mut candidate_entities: HashMap<EntityId, Option<u32>> =
        seed_ids.iter().map(|&id| (id, None)).collect();
    for (id, dist) in hop_distances {
        candidate_entities.entry(id).or_insert(Some(dist));
    }

    let mut seen_chunks: HashSet<ChunkId> = HashSet::new();
    let mut scored: Vec<(f64, ResultItem)> = Vec::new();

    for (entity_id, hop) in candidate_entities {
        let Some((chunk_id, doc_id, page_no)) = store.primary_definition(entity_id)? else { continue };
        if !seen_chunks.insert(chunk_id) {
            continue;
        }
        let entity = store.get_entity(entity_id)?;
        if let Some(kind_filter) = &opts.kind {
            if entity.kind.as_str() != *kind_filter {
                continue;
            }
        }
        let chunk = store.get_chunk(chunk_id)?;
        let doc = store.get_document(doc_id)?;
        let bm25 = fts_hits
            .iter()
            .find(|h| h.chunk_id == chunk_id)
            .map(|h| normalize_bm25(h.bm25))
            .unwrap_or(0.0);
        let score = combine_score(bm25, name_match_ids.contains(&entity_id), entity.centrality, hop, &weights);
        scored.push((
            score,
            ResultItem {
                entity_name: Some(entity.name),
                entity_kind: Some(entity.kind.as_str()),
                chunk_kind: chunk.kind,
                text: chunk.text,
                citation: Citation { document: doc.title, page: page_no },
                score,
            },
        ));
    }

    // Raw FTS hits not already covered by some entity's own definition
    // -- prose that mentions the query without itself defining anything.
    if opts.kind.is_none() {
        for hit in &fts_hits {
            if !seen_chunks.insert(hit.chunk_id) {
                continue;
            }
            let chunk = store.get_chunk(hit.chunk_id)?;
            let doc = store.get_document(chunk.doc_id)?;
            let score = combine_score(normalize_bm25(hit.bm25), false, 0.0, None, &weights);
            scored.push((
                score,
                ResultItem {
                    entity_name: None,
                    entity_kind: None,
                    chunk_kind: chunk.kind,
                    text: chunk.text,
                    citation: Citation { document: doc.title, page: chunk.start_page },
                    score,
                },
            ));
        }
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    let mut items = Vec::new();
    let mut total_tokens = 0usize;
    for (_, item) in scored {
        let tokens = (item.text.len() / 4).max(1);
        if !items.is_empty() && total_tokens + tokens > opts.budget_tokens {
            break;
        }
        total_tokens += tokens;
        items.push(item);
        if total_tokens >= opts.budget_tokens {
            break;
        }
    }

    Ok(ExploreResult { query: query.to_string(), items, total_tokens })
}
