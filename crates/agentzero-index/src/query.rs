use crate::embed::EmbeddedChunk;

/// Result of a similarity query.
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub score: f32,
    pub chunk: EmbeddedChunk,
}

/// Find the top-k most similar chunks to the query embedding using cosine similarity.
pub fn top_k(query: &[f32], chunks: &[EmbeddedChunk], k: usize) -> Vec<QueryResult> {
    let mut scored: Vec<QueryResult> = chunks
        .iter()
        .map(|ec| QueryResult {
            score: cosine_similarity(query, &ec.embedding),
            chunk: ec.clone(),
        })
        .collect();

    // Sort descending by score
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(k);
    scored
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::TextChunk;
    use std::path::PathBuf;

    fn make_chunk(content: &str, embedding: Vec<f32>) -> EmbeddedChunk {
        EmbeddedChunk {
            chunk: TextChunk {
                source_path: PathBuf::from("test.txt"),
                content: content.to_string(),
                start_byte: 0,
                end_byte: content.len(),
                chunk_index: 0,
            },
            embedding,
        }
    }

    #[test]
    fn identical_vectors_score_one() {
        let score = cosine_similarity(&[1.0, 0.0, 0.0], &[1.0, 0.0, 0.0]);
        assert!((score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn orthogonal_vectors_score_zero() {
        let score = cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]);
        assert!(score.abs() < 1e-6);
    }

    #[test]
    fn top_k_returns_best_matches() {
        let chunks = vec![
            make_chunk("low", vec![0.0, 1.0, 0.0]),
            make_chunk("high", vec![1.0, 0.0, 0.0]),
            make_chunk("medium", vec![0.7, 0.7, 0.0]),
        ];

        let query = vec![1.0, 0.0, 0.0];
        let results = top_k(&query, &chunks, 2);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].chunk.chunk.content, "high");
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn top_k_with_empty_chunks() {
        let results = top_k(&[1.0, 0.0], &[], 5);
        assert!(results.is_empty());
    }
}
