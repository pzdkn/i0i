//! Reciprocal Rank Fusion (RFC 0076).
//!
//! Pure: rankings in, fused ordering out. No database, no model — which is why
//! the part of retrieval most likely to need tuning is testable in microseconds.

/// The RRF constant from the original paper.
///
/// It flattens the gap between ranks 1 and 2 relative to the gap between
/// "ranked at all" and "absent", which is the behaviour we want: two signals
/// agreeing on a chunk should outrank one signal's enthusiasm for another.
const RRF_K: f64 = 60.0;

/// One signal's opinion of one chunk.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Signal {
    /// 1-based position in that signal's own ranking.
    pub rank: usize,
    /// That signal's native score. Higher is better in both signals: BM25 is
    /// negated at the store boundary, and vector distance is negated here.
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fused {
    pub chunk_id: String,
    pub score: f64,
    pub lexical: Option<Signal>,
    pub semantic: Option<Signal>,
}

/// Fuse a lexical and a semantic ranking, best first.
///
/// Both inputs are `(chunk_id, native_score)` in their own rank order. RRF uses
/// only the *positions*: BM25 is unbounded and shifts with corpus statistics,
/// while vector distance is bounded and model-dependent, so any weighted blend
/// of the raw numbers would need a calibration nobody has measured — and it
/// would drift as the library grows. Ordering is the only part of each signal
/// that is comparable across both.
///
/// Native scores are carried through untouched so a UI can explain a hit.
pub fn reciprocal_rank_fusion(
    lexical: &[(String, f64)],
    semantic: &[(String, f64)],
) -> Vec<Fused> {
    let mut fused: Vec<Fused> = Vec::new();

    let mut absorb = |ranking: &[(String, f64)], is_lexical: bool| {
        for (index, (chunk_id, score)) in ranking.iter().enumerate() {
            let signal = Signal {
                rank: index + 1,
                score: *score,
            };
            let contribution = 1.0 / (RRF_K + signal.rank as f64);

            match fused.iter_mut().find(|entry| entry.chunk_id == *chunk_id) {
                Some(entry) => {
                    entry.score += contribution;
                    if is_lexical {
                        entry.lexical = Some(signal);
                    } else {
                        entry.semantic = Some(signal);
                    }
                }
                None => fused.push(Fused {
                    chunk_id: chunk_id.clone(),
                    score: contribution,
                    lexical: is_lexical.then_some(signal),
                    semantic: (!is_lexical).then_some(signal),
                }),
            }
        }
    };

    absorb(lexical, true);
    absorb(semantic, false);

    // Ties broken by chunk id so the ordering is deterministic. Two chunks can
    // tie exactly whenever both appear at the same rank in disjoint rankings,
    // and an unstable order there would make results flicker between calls.
    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.chunk_id.cmp(&b.chunk_id))
    });
    fused
}

/// A single ranking presented in the fused shape, for `Lexical`/`Semantic` mode.
///
/// Keeps `score` meaning "the number this response was sorted by" in every
/// mode, rather than an RRF value that would be an odd thing to report when
/// only one signal ran.
pub fn single_signal(ranking: &[(String, f64)], is_lexical: bool) -> Vec<Fused> {
    ranking
        .iter()
        .enumerate()
        .map(|(index, (chunk_id, score))| {
            let signal = Signal {
                rank: index + 1,
                score: *score,
            };
            Fused {
                chunk_id: chunk_id.clone(),
                score: *score,
                lexical: is_lexical.then_some(signal),
                semantic: (!is_lexical).then_some(signal),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranking(ids: &[&str]) -> Vec<(String, f64)> {
        ids.iter()
            .enumerate()
            .map(|(index, id)| (id.to_string(), 1.0 - index as f64 * 0.1))
            .collect()
    }

    #[test]
    fn agreement_beats_a_single_signals_enthusiasm() {
        // "b" is second in both; "a" is first in one and absent from the other.
        let lexical = ranking(&["a", "b"]);
        let semantic = ranking(&["c", "b"]);

        let fused = reciprocal_rank_fusion(&lexical, &semantic);

        assert_eq!(fused[0].chunk_id, "b", "both signals agreed on b");
        assert!(fused[0].lexical.is_some() && fused[0].semantic.is_some());
    }

    #[test]
    fn a_chunk_ranked_by_one_signal_still_appears() {
        let fused = reciprocal_rank_fusion(&ranking(&["a"]), &ranking(&["b"]));

        assert_eq!(fused.len(), 2);
        assert!(fused.iter().any(|f| f.chunk_id == "a" && f.semantic.is_none()));
        assert!(fused.iter().any(|f| f.chunk_id == "b" && f.lexical.is_none()));
    }

    #[test]
    fn disjoint_rankings_order_by_rank_then_id() {
        // Rank 1 in either signal contributes identically, so the tie-break
        // must be deterministic rather than insertion-ordered.
        let fused = reciprocal_rank_fusion(&ranking(&["z"]), &ranking(&["a"]));

        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].chunk_id, "a");
        assert!((fused[0].score - fused[1].score).abs() < f64::EPSILON);
    }

    #[test]
    fn rank_one_in_both_outranks_rank_one_in_one() {
        let fused = reciprocal_rank_fusion(&ranking(&["x", "y"]), &ranking(&["x", "z"]));

        assert_eq!(fused[0].chunk_id, "x");
        // Two rank-1 contributions.
        let expected = 2.0 / (RRF_K + 1.0);
        assert!((fused[0].score - expected).abs() < 1e-12);
    }

    #[test]
    fn native_scores_survive_fusion() {
        let lexical = vec![("a".to_string(), 4.25)];
        let semantic = vec![("a".to_string(), -0.75)];

        let fused = reciprocal_rank_fusion(&lexical, &semantic);

        assert_eq!(fused[0].lexical.unwrap().score, 4.25);
        assert_eq!(fused[0].semantic.unwrap().score, -0.75);
        // The fused score is RRF, not a blend of the natives.
        assert!((fused[0].score - 2.0 / (RRF_K + 1.0)).abs() < 1e-12);
    }

    #[test]
    fn empty_rankings_fuse_to_nothing() {
        assert!(reciprocal_rank_fusion(&[], &[]).is_empty());
    }

    #[test]
    fn one_empty_ranking_passes_the_other_through_in_order() {
        let fused = reciprocal_rank_fusion(&ranking(&["a", "b", "c"]), &[]);

        let ids: Vec<&str> = fused.iter().map(|f| f.chunk_id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    #[test]
    fn single_signal_reports_the_native_score() {
        let fused = single_signal(&[("a".to_string(), 3.5)], true);

        assert_eq!(fused[0].score, 3.5, "single-mode score is the native score");
        assert_eq!(fused[0].lexical.unwrap().rank, 1);
        assert!(fused[0].semantic.is_none());
    }
}
