//! Retrieval ranking utilities shared across search backends.
//!
//! The main export is [`reciprocal_rank_fusion`], which merges several independently
//! ranked result lists (e.g. BM25 keyword results + HNSW semantic results) into a
//! single combined ranking. RRF is a simple, robust, parameter-light fusion method
//! that outperforms individual ranking signals on most retrieval benchmarks.
//!
//! Reference: Cormack et al., "Reciprocal Rank Fusion outperforms Condorcet and
//! individual Rank Learning Methods" (SIGIR 2009).

/// Standard RRF smoothing constant. The paper uses `k = 60`.
pub const DEFAULT_RRF_K: usize = 60;

/// Merge several ranked result lists into one using reciprocal rank fusion.
///
/// Each input list is a ranking of item IDs from most to least relevant. The output
/// is a single ranking where each item's score is the sum of `1 / (k + rank_i)`
/// across every list it appears in (1-indexed ranks). Items appearing in multiple
/// lists bubble up; items appearing in only one list contribute a smaller score.
///
/// Ties broken by lowest input ID for determinism.
///
/// # Example
/// ```
/// use agentzero_core::search::reciprocal_rank_fusion;
///
/// let keyword = vec![1_i64, 2, 3]; // BM25 ranking
/// let semantic = vec![3_i64, 1, 4]; // HNSW ranking
/// let merged = reciprocal_rank_fusion(&[keyword, semantic], 60);
/// // Item 1 and 3 appear in both → they outrank items 2 and 4
/// assert!(merged.iter().position(|&(id, _)| id == 1).unwrap() < 2);
/// assert!(merged.iter().position(|&(id, _)| id == 3).unwrap() < 2);
/// ```
pub fn reciprocal_rank_fusion(rankings: &[Vec<i64>], k: usize) -> Vec<(i64, f32)> {
    use std::collections::HashMap;

    let mut scores: HashMap<i64, f32> = HashMap::new();
    for ranking in rankings {
        for (rank_zero_based, id) in ranking.iter().enumerate() {
            let rank = rank_zero_based + 1; // RRF uses 1-indexed ranks
            let contribution = 1.0_f32 / (k as f32 + rank as f32);
            *scores.entry(*id).or_insert(0.0) += contribution;
        }
    }

    let mut out: Vec<(i64, f32)> = scores.into_iter().collect();
    out.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_produces_empty_output() {
        let merged = reciprocal_rank_fusion(&[], DEFAULT_RRF_K);
        assert!(merged.is_empty());
    }

    #[test]
    fn single_list_preserves_order() {
        let list = vec![10, 20, 30];
        let merged = reciprocal_rank_fusion(&[list], DEFAULT_RRF_K);
        let ids: Vec<i64> = merged.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec![10, 20, 30]);
        // First-ranked item has the highest score.
        assert!(merged[0].1 > merged[1].1);
        assert!(merged[1].1 > merged[2].1);
    }

    #[test]
    fn items_in_both_lists_outrank_items_in_one() {
        let a = vec![1, 2, 3];
        let b = vec![3, 4, 5];
        let merged = reciprocal_rank_fusion(&[a, b], DEFAULT_RRF_K);
        // Item 3 is in both lists — should rank first.
        assert_eq!(merged[0].0, 3);
    }

    #[test]
    fn rrf_handles_disjoint_lists() {
        let a = vec![1, 2];
        let b = vec![3, 4];
        let merged = reciprocal_rank_fusion(&[a, b], DEFAULT_RRF_K);
        assert_eq!(merged.len(), 4);
        // Items at rank 1 in their respective lists should share the top positions.
        let top_two: Vec<i64> = merged.iter().take(2).map(|(id, _)| *id).collect();
        assert!(top_two.contains(&1));
        assert!(top_two.contains(&3));
    }

    #[test]
    fn higher_rank_produces_higher_score() {
        let list = vec![1, 2, 3, 4, 5];
        let merged = reciprocal_rank_fusion(&[list], DEFAULT_RRF_K);
        for pair in merged.windows(2) {
            assert!(pair[0].1 > pair[1].1);
        }
    }

    #[test]
    fn ties_broken_by_lowest_id() {
        // Two lists where items 5 and 10 appear at symmetric positions.
        let a = vec![5, 10];
        let b = vec![10, 5];
        let merged = reciprocal_rank_fusion(&[a, b], DEFAULT_RRF_K);
        assert_eq!(merged.len(), 2);
        // Both items have the same total score; tie broken by lowest id.
        assert_eq!(merged[0].0, 5);
        assert_eq!(merged[1].0, 10);
    }

    #[test]
    fn three_way_fusion() {
        let bm25 = vec![1, 2, 3];
        let hnsw = vec![2, 1, 4];
        let recency = vec![3, 2, 1];
        let merged = reciprocal_rank_fusion(&[bm25, hnsw, recency], DEFAULT_RRF_K);
        // Item 2 appears in all three lists at rank ≤ 2 — should rank first.
        assert_eq!(merged[0].0, 2);
    }
}
