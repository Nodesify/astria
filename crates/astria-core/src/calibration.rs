//! Cosine-scale calibration for the semantic layer.
//!
//! Cosine similarity values are model-specific: the same "related" level
//! sits at a different cosine for every embedding model, so every threshold
//! that compares raw cosines — the query noise floor, the seed-rescale
//! anchors, the reserved-seed floor, the `similar_to` edge threshold —
//! must come from one measured table instead of scattered constants.
//! Swapping the model without re-measuring silently strands the thresholds
//! on the old scale (measured on the Click corpus, 2,257 nodes: the best
//! relevant match for one heldout question lands at cosine 0.540 on
//! jina-v2-base-code — below a floor inherited from bge-small, which would
//! drop it before ranking ever sees it).
//!
//! The anchors below were measured with
//! `cargo run --release -p astria-embed --example calibrate` on the Click
//! graph against the frozen + heldout golden questions (never the reserved
//! sets). Keep the example run whenever `astria_embed::MODEL` changes, and
//! re-measure rather than assuming any anchor transfers.

/// Measured cosine anchors for one embedding model, plus the derived
/// seed-score mapping. All fields are raw cosines on this model's scale.
pub struct SemanticCalibration {
    /// The model these anchors were measured on (`astria_embed::MODEL_NAME`).
    pub model: &'static str,
    /// Raw cosine below which a query→node match is noise: filtered from
    /// semantic candidates and scored as zero seed evidence. Measured just
    /// under the per-question noise p95 band, below the weakest observed
    /// relevant match.
    pub noise_floor: f64,
    /// Raw cosine of a strong query→node match (the observed median best
    /// relevant match); maps to `seed_score_cap`.
    pub strong_match: f64,
    /// Ceiling for a rescaled semantic seed score — just under an exact
    /// label match, so token evidence always outranks pure embedding recall.
    pub seed_score_cap: f64,
    /// Raw cosine a semantic-only candidate must clear to reserve a seed
    /// slot: above the noise p95 band, at the low edge of the relevant
    /// range — a reservation must be genuinely about the question, not a
    /// distant neighbor.
    pub seed_slot_floor: f64,
    /// Raw cosine for a node→node `similar_to` edge. Chosen to preserve the
    /// edge density of the previous model's threshold (jina's nearest
    /// neighbors sit lower than bge's; at bge's 0.80 more than half of all
    /// nodes would get no `similar_to` edge at all).
    pub similar_to_threshold: f64,
}

impl SemanticCalibration {
    /// Map a raw query→node cosine into the query engine's seed-score
    /// scale: `noise_floor` scores 0, `strong_match` and above score
    /// `seed_score_cap`.
    pub const fn seed_score(&self, cosine: f64) -> f64 {
        if cosine <= self.noise_floor {
            return 0.0;
        }
        let scale = self.seed_score_cap / (self.strong_match - self.noise_floor);
        let score = (cosine - self.noise_floor) * scale;
        if score > self.seed_score_cap {
            self.seed_score_cap
        } else {
            score
        }
    }

    /// Seed-score equivalent of `seed_slot_floor` — the reservation check
    /// runs in seed-score units.
    pub const fn seed_slot_score_floor(&self) -> f64 {
        self.seed_score(self.seed_slot_floor)
    }

    /// Seed-score mapping for description-shaped questions — the ones whose
    /// identifying (salient) terms have no lexical evidence anywhere in the
    /// graph, so lexical ranking is admitted to be guessing among partial
    /// matches. There a strong calibrated cosine is the only real evidence
    /// about the answer and may rank like a full label match instead of
    /// merely breaking ties: `strong_match` maps to the label-match tier
    /// (2.0 on the lexical scale, where a full term coverage scores 2.0 and
    /// an exact label match 4.0), rising to 2.6 at the top of the scale.
    /// Cosines below `strong_match` keep the tie-breaking cap — the boost
    /// is for evidence the calibration calls a strong match, not for
    /// neighbors, and an exact label match still outranks everything.
    pub const fn description_seed_score(&self, cosine: f64) -> f64 {
        if cosine < self.strong_match {
            return self.seed_score(cosine);
        }
        let score = 2.0 + (cosine - self.strong_match) * 3.0;
        if score > 2.6 {
            2.6
        } else {
            score
        }
    }
}

/// Calibration for the shipped model (jina-embeddings-v2-base-code).
///
/// Measured 2026-09-30 on the Click corpus (2,257 nodes, 10 frozen/heldout
/// golden questions): relevant best matches 0.540–0.855 (median 0.697),
/// noise p50 ≈ 0.20, p95 0.37–0.52; node nearest-neighbor median 0.783
/// with bge-density edge parity at 0.75. The previous bge-small anchors
/// (noise floor 0.55, slot floor ≈ 0.63, similar_to 0.80) live on a
/// different, compressed scale and must not be reused.
pub const SEMANTIC_CALIBRATION: SemanticCalibration = SemanticCalibration {
    model: "jinaai/jina-embeddings-v2-base-code",
    noise_floor: 0.45,
    strong_match: 0.70,
    seed_score_cap: 0.67,
    seed_slot_floor: 0.52,
    similar_to_threshold: 0.75,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_score_maps_the_measured_anchors() {
        let c = &SEMANTIC_CALIBRATION;
        assert_eq!(c.seed_score(0.0), 0.0);
        assert_eq!(c.seed_score(c.noise_floor), 0.0);
        // below the floor: no evidence, even close to it
        assert_eq!(c.seed_score(c.noise_floor - 0.01), 0.0);
        // strong match reaches the cap, and so does everything above it
        assert!((c.seed_score(c.strong_match) - c.seed_score_cap).abs() < 1e-9);
        assert!((c.seed_score(1.0) - c.seed_score_cap).abs() < 1e-9);
        // monotone between the anchors
        assert!(c.seed_score(0.50) < c.seed_score(0.60));
        assert!(c.seed_score(0.60) < c.seed_score(c.strong_match));
    }

    #[test]
    fn slot_floor_sits_between_noise_and_relevant_band() {
        // The measured bands: noise p95 up to 0.52, weakest relevant match
        // 0.540 — the slot floor must thread them, and its seed-score form
        // must stay reachable by the reservation.
        let c = &SEMANTIC_CALIBRATION;
        assert!(c.seed_slot_floor >= 0.50 && c.seed_slot_floor <= 0.54);
        assert!(c.seed_slot_score_floor() > 0.0);
        assert!(c.seed_slot_score_floor() < c.seed_score_cap);
    }

    #[test]
    fn description_scores_outrank_partial_matches_not_exact_labels() {
        let c = &SEMANTIC_CALIBRATION;
        // Below strong_match: identical to the tie-breaking mapping.
        assert_eq!(
            c.description_seed_score(c.noise_floor),
            c.seed_score(c.noise_floor)
        );
        assert_eq!(
            c.description_seed_score(c.strong_match - 0.01),
            c.seed_score(c.strong_match - 0.01)
        );
        // At/above strong_match: the label-match tier (a full term coverage
        // scores 2.0 on the lexical scale), capped below an exact label
        // match (4.0) so token evidence that does exist still wins.
        assert_eq!(c.description_seed_score(c.strong_match), 2.0);
        assert!(c.description_seed_score(c.strong_match + 0.1) > 2.0);
        assert_eq!(c.description_seed_score(1.0), 2.6);
        assert!(c.description_seed_score(1.0) < 4.0);
    }
}
