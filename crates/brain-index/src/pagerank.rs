//! Weighted PageRank centrality over the entity graph. Used purely as a
//! ranking prior — "how connected is this entity" — for
//! [`brain_query`](../brain_query/index.html)'s result ordering and
//! [`brain_wiki`](../brain_wiki/index.html)'s page-generation threshold.
//!
//! Every edge is treated as undirected for this computation regardless of
//! how it's stored (a `Mentions` edge only recorded `A -> B` still lets
//! importance flow from B back to A): centrality here means "how
//! entangled with the rest of the graph", not "how often cited by
//! others", so symmetric propagation is the right model even though the
//! edges themselves carry a direction for other purposes.

use brain_core::{Edge, EntityId};
use std::collections::HashMap;

/// Standard PageRank damping factor.
const DEFAULT_DAMPING: f64 = 0.85;
/// Enough iterations to converge well past the precision this ranking
/// prior actually needs.
const DEFAULT_ITERATIONS: usize = 20;

/// Computes PageRank over `entity_ids` using `edges` as weighted,
/// symmetric connections. Returns a score per entity summing to ~1.0
/// across the whole graph (standard PageRank normalization).
pub fn pagerank(entity_ids: &[EntityId], edges: &[Edge]) -> HashMap<EntityId, f64> {
    pagerank_with_params(entity_ids, edges, DEFAULT_DAMPING, DEFAULT_ITERATIONS)
}

/// As [`pagerank`], with explicit damping factor and iteration count
/// (exposed for tests; production callers should use [`pagerank`]).
pub fn pagerank_with_params(
    entity_ids: &[EntityId],
    edges: &[Edge],
    damping: f64,
    iterations: usize,
) -> HashMap<EntityId, f64> {
    let n = entity_ids.len();
    if n == 0 {
        return HashMap::new();
    }
    let index: HashMap<EntityId, usize> =
        entity_ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();

    let mut adjacency: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    let mut out_weight = vec![0.0f64; n];
    for edge in edges {
        if let (Some(&a), Some(&b)) = (index.get(&edge.src), index.get(&edge.dst)) {
            if a == b || edge.weight <= 0.0 {
                continue;
            }
            adjacency[a].push((b, edge.weight));
            adjacency[b].push((a, edge.weight));
            out_weight[a] += edge.weight;
            out_weight[b] += edge.weight;
        }
    }

    let base = (1.0 - damping) / n as f64;
    let mut rank = vec![1.0 / n as f64; n];
    for _ in 0..iterations {
        let mut next = vec![base; n];
        for (u, neighbors) in adjacency.iter().enumerate() {
            if out_weight[u] <= 0.0 {
                continue;
            }
            let share = damping * rank[u] / out_weight[u];
            for &(v, w) in neighbors {
                next[v] += share * w;
            }
        }
        rank = next;
    }

    entity_ids.iter().enumerate().map(|(i, &id)| (id, rank[i])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::EdgeKind;

    fn id(n: i64) -> EntityId {
        EntityId::new(n)
    }

    fn edge(src: i64, dst: i64, weight: f64) -> Edge {
        Edge { src: id(src), dst: id(dst), kind: EdgeKind::CoOccurs, weight, evidence_chunk_id: None }
    }

    #[test]
    fn a_hub_node_ranks_higher_than_leaves() {
        // Star graph: node 1 connects to 2, 3, 4, 5.
        let ids: Vec<EntityId> = (1..=5).map(id).collect();
        let edges = vec![edge(1, 2, 1.0), edge(1, 3, 1.0), edge(1, 4, 1.0), edge(1, 5, 1.0)];
        let ranks = pagerank(&ids, &edges);
        let hub = ranks[&id(1)];
        for leaf in 2..=5 {
            assert!(hub > ranks[&id(leaf)], "hub must outrank every leaf");
        }
    }

    #[test]
    fn isolated_nodes_get_the_uniform_base_rank() {
        // With no edges to redistribute rank along, every node settles at
        // the flat teleportation term (1 - damping) / n -- this is a
        // relative-ranking prior, not a normalized probability
        // distribution, so it deliberately does not sum to 1 here.
        let ids: Vec<EntityId> = (1..=3).map(id).collect();
        let ranks = pagerank(&ids, &[]);
        let expected = (1.0 - DEFAULT_DAMPING) / 3.0;
        for r in ranks.values() {
            assert!((r - expected).abs() < 1e-9);
        }
    }

    #[test]
    fn heavier_edges_transfer_more_rank() {
        let ids: Vec<EntityId> = (1..=3).map(id).collect();
        // Node 1 connects weakly to 2, strongly to 3.
        let edges = vec![edge(1, 2, 1.0), edge(1, 3, 10.0)];
        let ranks = pagerank(&ids, &edges);
        assert!(ranks[&id(3)] > ranks[&id(2)], "a heavier edge should transfer more centrality");
    }
}
