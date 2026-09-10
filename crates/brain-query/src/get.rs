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
    let mut neighbours = Vec::new();
    for edge in edges.into_iter().take(MAX_NEIGHBOURS) {
        let other_id = if edge.src == entity_id { edge.dst } else { edge.src };
        let other = store.get_entity(other_id)?;
        neighbours.push(Neighbour { id: other_id, name: other.name, kind: other.kind.as_str(), weight: edge.weight });
    }

    Ok(Some(EntityView { name: entity.name, kind: entity.kind.as_str(), definition, citation, fields, neighbours }))
}
