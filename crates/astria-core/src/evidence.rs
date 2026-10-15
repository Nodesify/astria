//! Evidence weights: the single strength ordering shared by query scoring
//! and clustering, so "how strongly is this fact supported" means the same
//! thing everywhere. Alphabetical comparison of confidence labels does NOT
//! order by strength ("SEMANTIC" > "LLM" lexicographically), so labels map
//! to numbers.

/// Numeric rank of a confidence label. EXTRACTED/DECLARED source facts are
/// the strongest structural evidence; RESOLVED is a call extracted from
/// source whose bare name bound to exactly one definition (below the
/// EXTRACTED/DECLARED tier so "compiler-grade facts only" filters still
/// exclude it); INFERRED is reconstruction from name references; SEMANTIC
/// is embedding similarity.
pub fn confidence_rank(confidence: &str) -> f64 {
    match confidence.to_uppercase().as_str() {
        "DECLARED" => 1.0,
        "EXTRACTED" => 0.9,
        "RESOLVED" => 0.85,
        "INFERRED" => 0.7,
        "SEMANTIC" => 0.6,
        _ => 0.5,
    }
}

/// Effective strength of an edge: the stored numeric score when it is a
/// usable number, otherwise the rank of its confidence label. Used as the
/// edge weight wherever evidence strength should shape results — module
/// boundaries in clustering above all.
pub fn edge_weight(confidence: &str, confidence_score: Option<f64>) -> f64 {
    match confidence_score {
        Some(score) if (0.0..=1.0).contains(&score) => score,
        _ => confidence_rank(confidence),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_rank_by_evidence_not_alphabetically() {
        assert!(confidence_rank("DECLARED") > confidence_rank("EXTRACTED"));
        assert!(confidence_rank("EXTRACTED") > confidence_rank("RESOLVED"));
        assert!(confidence_rank("RESOLVED") > confidence_rank("INFERRED"));
        assert!(confidence_rank("SEMANTIC") < confidence_rank("INFERRED"));
    }

    #[test]
    fn stored_score_wins_when_usable() {
        assert_eq!(edge_weight("EXTRACTED", Some(0.75)), 0.75);
        // Out-of-range or missing scores fall back to the label rank.
        assert_eq!(edge_weight("EXTRACTED", Some(1.5)), 0.9);
        assert_eq!(edge_weight("EXTRACTED", None), 0.9);
        assert_eq!(edge_weight("whatever", None), 0.5);
    }
}
