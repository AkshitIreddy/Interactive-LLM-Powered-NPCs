use crate::MemoryRecord;
use std::collections::HashMap;

const RRF_K: f64 = 60.0;

/// Rank positions are zero-based. Missing modalities contribute no RRF score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CandidateRanks {
    pub lexical: Option<usize>,
    pub semantic: Option<usize>,
}

/// Deterministically combines retrieval modalities with durable memory signals.
/// The final ID comparison ensures stable output even when every score ties.
pub fn rank_candidates(
    mut candidates: Vec<(MemoryRecord, CandidateRanks)>,
    now_ms: i64,
) -> Vec<(MemoryRecord, f64, CandidateRanks)> {
    let mut ranked = candidates
        .drain(..)
        .map(|(record, ranks)| {
            let lexical = ranks
                .lexical
                .map(|rank| 1.0 / (RRF_K + rank as f64 + 1.0))
                .unwrap_or(0.0);
            let semantic = ranks
                .semantic
                .map(|rank| 1.0 / (RRF_K + rank as f64 + 1.0))
                .unwrap_or(0.0);
            let age_days =
                (now_ms.saturating_sub(record.observed_at_ms).max(0) as f64) / 86_400_000.0;
            let recency = 1.0 / (1.0 + age_days / 30.0);
            let score = lexical * 0.50
                + semantic * 0.40
                + record.importance * 0.0008
                + record.confidence * 0.0004
                + recency * 0.0002;
            (record, score, ranks)
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| right.0.importance.total_cmp(&left.0.importance))
            .then_with(|| right.0.observed_at_ms.cmp(&left.0.observed_at_ms))
            .then_with(|| left.0.id.cmp(&right.0.id))
    });
    ranked
}

pub(crate) fn rank_map(ids: impl IntoIterator<Item = String>) -> HashMap<String, usize> {
    ids.into_iter()
        .enumerate()
        .map(|(rank, id)| (id, rank))
        .collect()
}

pub(crate) fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f64> {
    if left.len() != right.len() || left.is_empty() {
        return None;
    }
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for (&a, &b) in left.iter().zip(right) {
        let a = f64::from(a);
        let b = f64::from(b);
        dot += a * b;
        left_norm += a * a;
        right_norm += b * b;
    }
    let denominator = left_norm.sqrt() * right_norm.sqrt();
    (denominator > f64::EPSILON).then_some(dot / denominator)
}
