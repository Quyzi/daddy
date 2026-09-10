//! Graph-walk expansion: breadth-first search outward from a set of seed
//! entities along stored edges, up to a hop limit.

use brain_core::error::Result;
use brain_core::EntityId;
use brain_store::Store;
use std::collections::{HashMap, HashSet, VecDeque};

/// Expands from `seeds` up to `max_hops` hops via `store`'s edges
/// (treated as undirected — see `brain-index`'s `pagerank` module docs
/// for the same reasoning applied to centrality). Returns each reached
/// non-seed entity's minimum hop distance; seeds themselves are not
/// included (callers already know about them directly, at hop 0).
pub fn expand(store: &Store, seeds: &[EntityId], max_hops: u32) -> Result<HashMap<EntityId, u32>> {
    let mut distance: HashMap<EntityId, u32> = HashMap::new();
    let mut seen: HashSet<EntityId> = seeds.iter().copied().collect();
    let mut frontier: VecDeque<(EntityId, u32)> = seeds.iter().map(|&s| (s, 0)).collect();

    while let Some((id, d)) = frontier.pop_front() {
        if d >= max_hops {
            continue;
        }
        for edge in store.edges_touching(id)? {
            let other = if edge.src == id { edge.dst } else { edge.src };
            if seen.insert(other) {
                distance.insert(other, d + 1);
                frontier.push_back((other, d + 1));
            }
        }
    }
    Ok(distance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::{Edge, EdgeKind, Entity, EntityKind};

    fn make_entity(store: &Store, name: &str) -> EntityId {
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
    fn expands_two_hops_and_respects_the_limit() {
        let store = Store::open_in_memory().unwrap();
        let a = make_entity(&store, "A");
        let b = make_entity(&store, "B");
        let c = make_entity(&store, "C");
        let d = make_entity(&store, "D");
        // Chain: A - B - C - D
        for (src, dst) in [(a, b), (b, c), (c, d)] {
            store
                .add_edge(&Edge { src, dst, kind: EdgeKind::CoOccurs, weight: 1.0, evidence_chunk_id: None })
                .unwrap();
        }

        let one_hop = expand(&store, &[a], 1).unwrap();
        assert_eq!(one_hop.get(&b), Some(&1));
        assert!(!one_hop.contains_key(&c), "C is 2 hops away, must not appear within a 1-hop limit");

        let two_hop = expand(&store, &[a], 2).unwrap();
        assert_eq!(two_hop.get(&b), Some(&1));
        assert_eq!(two_hop.get(&c), Some(&2));
        assert!(!two_hop.contains_key(&d), "D is 3 hops away, must not appear within a 2-hop limit");
    }

    #[test]
    fn multiple_seeds_each_contribute_their_own_frontier() {
        let store = Store::open_in_memory().unwrap();
        let a = make_entity(&store, "A");
        let b = make_entity(&store, "B");
        let x = make_entity(&store, "X");
        let y = make_entity(&store, "Y");
        store.add_edge(&Edge { src: a, dst: b, kind: EdgeKind::CoOccurs, weight: 1.0, evidence_chunk_id: None }).unwrap();
        store.add_edge(&Edge { src: x, dst: y, kind: EdgeKind::CoOccurs, weight: 1.0, evidence_chunk_id: None }).unwrap();

        let reached = expand(&store, &[a, x], 1).unwrap();
        assert_eq!(reached.get(&b), Some(&1));
        assert_eq!(reached.get(&y), Some(&1));
    }
}
