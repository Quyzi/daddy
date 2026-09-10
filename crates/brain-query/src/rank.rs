//! Pure ranking math: combines an FTS5 BM25 score, a name-match boost,
//! graph centrality, and hop-distance decay into one comparable score.
//! Kept free of any database access so the weighting itself is testable
//! in isolation from retrieval plumbing.

/// Weights for each signal in [`combine_score`]. The defaults favor exact
/// text/name matches over centrality — a highly-connected entity that
/// the query doesn't actually mention shouldn't outrank one it does.
#[derive(Debug, Clone, Copy)]
pub struct ScoreWeights {
    /// Weight applied to the (already normalized-positive) BM25 score.
    pub bm25: f64,
    /// Flat bonus added when the query matches an entity's name.
    pub name_match: f64,
    /// Weight applied to PageRank centrality.
    pub centrality: f64,
    /// Multiplicative decay applied per graph hop away from a seed
    /// match (a direct FTS/name hit is hop 0 and undecayed).
    pub hop_decay: f64,
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self { bm25: 1.0, name_match: 2.0, centrality: 50.0, hop_decay: 0.5 }
    }
}

/// Converts SQLite FTS5's `bm25()` output (more negative = more
/// relevant) into a positive "goodness" score (larger = more relevant),
/// clamping at zero so a non-hit never contributes a negative score.
pub fn normalize_bm25(raw: f64) -> f64 {
    (-raw).max(0.0)
}

/// Combines every signal into one score. `hops` is `None` for a direct
/// seed match (FTS or name hit) and `Some(n)` for an entity reached by
/// `n` graph hops from a seed.
pub fn combine_score(
    bm25_component: f64,
    name_match: bool,
    centrality: f64,
    hops: Option<u32>,
    weights: &ScoreWeights,
) -> f64 {
    let mut score = weights.bm25 * bm25_component + weights.centrality * centrality;
    if name_match {
        score += weights.name_match;
    }
    if let Some(h) = hops {
        score *= weights.hop_decay.powi(h as i32);
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_bm25_flips_sign_and_clamps_at_zero() {
        assert_eq!(normalize_bm25(-3.5), 3.5);
        assert_eq!(normalize_bm25(0.0), 0.0);
        assert_eq!(normalize_bm25(2.0), 0.0, "a non-hit must never contribute a negative score");
    }

    #[test]
    fn a_direct_text_match_outranks_an_unrelated_but_central_entity() {
        let weights = ScoreWeights::default();
        let direct_match = combine_score(5.0, false, 0.0001, None, &weights);
        let central_but_irrelevant = combine_score(0.0, false, 0.05, None, &weights);
        assert!(direct_match > central_but_irrelevant);
    }

    #[test]
    fn hop_decay_reduces_score_monotonically_with_distance() {
        let weights = ScoreWeights::default();
        let hop0 = combine_score(0.0, false, 0.01, Some(0), &weights);
        let hop1 = combine_score(0.0, false, 0.01, Some(1), &weights);
        let hop2 = combine_score(0.0, false, 0.01, Some(2), &weights);
        assert!(hop0 > hop1);
        assert!(hop1 > hop2);
    }

    #[test]
    fn name_match_adds_a_flat_bonus() {
        let weights = ScoreWeights::default();
        let with_name = combine_score(0.0, true, 0.0, None, &weights);
        let without = combine_score(0.0, false, 0.0, None, &weights);
        assert!(with_name > without);
    }
}
