//! Derives graph edges from the entities mentioned within a single
//! chunk: every distinct pair co-occurs, and if the chunk is itself an
//! entity's primary definition, that entity gets a directed `Mentions`
//! edge to everything else it mentions.

use brain_core::{Edge, EdgeKind, EntityId};

/// Computes the edges implied by one chunk's set of (deduplicated)
/// mentioned entities. `defining_entity` is the entity this chunk is the
/// primary definition of, if any (see
/// [`brain_store::Store::defining_entity_for_chunk`]).
///
/// Each returned edge has `weight: 1.0` — callers apply them via
/// [`brain_store::Store::add_edge`], which accumulates weight on
/// conflict, so repeated co-occurrence across many chunks naturally adds
/// up to a stronger edge without this function needing to track running
/// totals itself.
pub fn derive_edges_for_chunk(mentioned: &[EntityId], defining_entity: Option<EntityId>) -> Vec<Edge> {
    let mut unique: Vec<EntityId> = mentioned.to_vec();
    unique.sort_by_key(|e| e.get());
    unique.dedup();

    let mut edges = Vec::new();
    for i in 0..unique.len() {
        for j in (i + 1)..unique.len() {
            edges.push(Edge {
                src: unique[i],
                dst: unique[j],
                kind: EdgeKind::CoOccurs,
                weight: 1.0,
                evidence_chunk_id: None,
            });
        }
    }

    if let Some(def) = defining_entity {
        for &e in &unique {
            if e != def {
                edges.push(Edge {
                    src: def,
                    dst: e,
                    kind: EdgeKind::Mentions,
                    weight: 1.0,
                    evidence_chunk_id: None,
                });
            }
        }
    }

    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: i64) -> EntityId {
        EntityId::new(n)
    }

    #[test]
    fn every_distinct_pair_co_occurs_exactly_once() {
        let edges = derive_edges_for_chunk(&[id(1), id(2), id(3)], None);
        assert_eq!(edges.len(), 3, "3 entities -> C(3,2) = 3 co-occurrence pairs");
        assert!(edges.iter().all(|e| e.kind == EdgeKind::CoOccurs));
    }

    #[test]
    fn defining_entity_gets_directed_mentions_edges_excluding_itself() {
        let edges = derive_edges_for_chunk(&[id(1), id(2), id(3)], Some(id(1)));
        let mentions: Vec<&Edge> = edges.iter().filter(|e| e.kind == EdgeKind::Mentions).collect();
        assert_eq!(mentions.len(), 2, "entity 1 mentions 2 and 3, but never itself");
        assert!(mentions.iter().all(|e| e.src == id(1)));
        assert!(mentions.iter().any(|e| e.dst == id(2)));
        assert!(mentions.iter().any(|e| e.dst == id(3)));
    }

    #[test]
    fn duplicate_mentions_of_the_same_entity_collapse_before_pairing() {
        let edges = derive_edges_for_chunk(&[id(1), id(1), id(2)], None);
        assert_eq!(edges.len(), 1, "repeated mentions of the same entity must not create self-pairs");
    }

    #[test]
    fn single_mention_produces_no_edges() {
        assert!(derive_edges_for_chunk(&[id(1)], None).is_empty());
        assert!(derive_edges_for_chunk(&[], None).is_empty());
    }
}
