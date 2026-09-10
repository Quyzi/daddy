//! `brain get <entity>`: a verbatim, no-ranking lookup of one entity's
//! definition, structured fields, and graph neighbourhood.

use crate::types::{Citation, EntityView, FieldValue, Neighbour};
use brain_core::error::Result;
use brain_store::Store;

/// Maximum neighbours returned, heaviest edge first.
const MAX_NEIGHBOURS: usize = 20;

/// Looks up an entity by exact slug first (so `brain get fireball` is
/// unambiguous even if "fireball" also appears as a name substring
/// elsewhere), falling back to a name search for a looser match.
/// Returns `Ok(None)` if nothing matches at all.
pub fn get_entity(store: &Store, name_or_slug: &str) -> Result<Option<EntityView>> {
    let entity = match store.find_entity_by_slug(&brain_core::slugify(name_or_slug))? {
        Some(e) => e,
        None => match store.search_entities_by_name(name_or_slug, 1)?.into_iter().next() {
            Some(e) => e,
            None => return Ok(None),
        },
    };
    let entity_id = entity.id.expect("entities read back from storage always have an id");

    let (definition, citation) = match store.primary_definition(entity_id)? {
        Some((chunk_id, doc_id, page_no)) => {
            let chunk = store.get_chunk(chunk_id)?;
            let doc = store.get_document(doc_id)?;
            (Some(chunk.text), Some(Citation { document: doc.title, page: page_no }))
        }
        None => (None, None),
    };

    let mut fields = Vec::new();
    for f in store.list_fields(entity_id)? {
        let document = store.get_document(f.doc_id).map(|d| d.title).unwrap_or_default();
        fields.push(FieldValue { key: f.key, value: f.value, document });
    }

    let mut edges = store.edges_touching(entity_id)?;
    edges.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
    // Two entities can be connected by more than one edge *kind* at once
    // (e.g. a `[[wikilink]]` to another note that's also separately
    // picked up as a gazetteer mention gets both a `LinksTo` edge and a
    // `Mentions`/`CoOccurs` edge to the same entity) — list each
    // neighbour once, at its strongest connection, not once per kind.
    let mut seen = std::collections::HashSet::new();
    let mut neighbours = Vec::new();
    for edge in edges {
        let other_id = if edge.src == entity_id { edge.dst } else { edge.src };
        if !seen.insert(other_id) {
            continue;
        }
        let other = store.get_entity(other_id)?;
        neighbours.push(Neighbour { id: other_id, name: other.name, kind: other.kind.as_str(), weight: edge.weight });
        if neighbours.len() >= MAX_NEIGHBOURS {
            break;
        }
    }

    Ok(Some(EntityView { name: entity.name, kind: entity.kind.as_str(), definition, citation, fields, neighbours }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::{Edge, EdgeKind, Entity, EntityKind};

    fn make_entity(store: &Store, name: &str) -> brain_core::EntityId {
        store
            .upsert_entity(&Entity {
                id: None,
                kind: EntityKind::Topic,
                name: name.to_string(),
                slug: brain_core::slugify(name),
                canonical_id: None,
                centrality: 0.0,
                confidence: 1.0,
            })
            .unwrap()
    }

    #[test]
    fn neighbours_lists_an_entity_connected_by_multiple_edge_kinds_only_once() {
        let store = Store::open_in_memory().unwrap();
        let barovia = make_entity(&store, "Barovia");
        let strahd = make_entity(&store, "Strahd");

        // The same pair, connected three different ways -- exactly what
        // a `[[wikilink]]`ed note that's also gazetteer-mentioned in the
        // same text produces.
        for kind in [EdgeKind::LinksTo, EdgeKind::Mentions, EdgeKind::CoOccurs] {
            store
                .add_edge(&Edge { src: barovia, dst: strahd, kind, weight: 1.0, evidence_chunk_id: None })
                .unwrap();
        }

        let view = get_entity(&store, "Barovia").unwrap().expect("barovia should exist");
        let strahd_count = view.neighbours.iter().filter(|n| n.name == "Strahd").count();
        assert_eq!(strahd_count, 1, "Strahd should appear once regardless of how many edge kinds connect it");
    }
}
