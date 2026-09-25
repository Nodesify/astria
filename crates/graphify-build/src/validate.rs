// validate: structural checks on extractions before they enter the graph.
// Fail loud with ALL violations at once, not first-only. Dangling edge
// endpoints are NOT violations — build() creates stub nodes for them by
// design (unresolved cross-file calls are normal) — but empty ids, labels,
// relations, unknown confidence values, and duplicate node ids are data
// corruption and must stop the run.

use graphify_extract::Extraction;

pub struct ValidationIssue {
    pub file: String,
    pub kind: String,
    pub detail: String,
}

impl std::fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}: {}", self.file, self.kind, self.detail)
    }
}

const CONFIDENCE_VALUES: &[&str] = &["EXTRACTED", "INFERRED", "AMBIGUOUS"];

pub fn validate_extractions(extractions: &[Extraction]) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    for extraction in extractions {
        let file = extraction.file_path.display().to_string();
        let mut seen_ids = std::collections::HashSet::new();
        for node in &extraction.nodes {
            if node.id.trim().is_empty() {
                issues.push(ValidationIssue {
                    file: file.clone(),
                    kind: "node".into(),
                    detail: "node with empty id".into(),
                });
                continue;
            }
            if node.label.trim().is_empty() {
                issues.push(ValidationIssue {
                    file: file.clone(),
                    kind: "node".into(),
                    detail: format!("node '{}' has an empty label", node.id),
                });
            }
            if node.source_file.as_os_str().is_empty() {
                issues.push(ValidationIssue {
                    file: file.clone(),
                    kind: "node".into(),
                    detail: format!("node '{}' has an empty source_file", node.id),
                });
            }
            if !seen_ids.insert(node.id.as_str()) {
                issues.push(ValidationIssue {
                    file: file.clone(),
                    kind: "node".into(),
                    detail: format!("duplicate node id '{}' in one extraction", node.id),
                });
            }
        }
        for edge in &extraction.edges {
            if edge.source.trim().is_empty() || edge.target.trim().is_empty() {
                issues.push(ValidationIssue {
                    file: file.clone(),
                    kind: "edge".into(),
                    detail: format!(
                        "edge '{}' -> '{}' has an empty endpoint",
                        edge.source, edge.target
                    ),
                });
                continue;
            }
            if edge.relation.trim().is_empty() {
                issues.push(ValidationIssue {
                    file: file.clone(),
                    kind: "edge".into(),
                    detail: format!(
                        "edge '{}' -> '{}' has an empty relation",
                        edge.source, edge.target
                    ),
                });
            }
            if !CONFIDENCE_VALUES.contains(&edge.confidence.as_str()) {
                issues.push(ValidationIssue {
                    file: file.clone(),
                    kind: "edge".into(),
                    detail: format!(
                        "edge '{}' -> '{}' has unknown confidence '{}'",
                        edge.source, edge.target, edge.confidence
                    ),
                });
            }
        }
    }
    issues
}

/// Validate and return a single combined error listing every violation.
pub fn assert_valid(extractions: &[Extraction]) -> graphify_core::Result<()> {
    let issues = validate_extractions(extractions);
    if issues.is_empty() {
        return Ok(());
    }
    let detail = issues
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    Err(graphify_core::GraphifyError::Graph(format!(
        "extraction validation failed ({} issue{}): {}",
        issues.len(),
        if issues.len() == 1 { "" } else { "s" },
        detail
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphify_extract::{ExtractedEdge, ExtractedNode, Extraction};
    use std::path::PathBuf;

    fn ext(id: &str, label: &str, edges: Vec<(&str, &str, &str, &str)>) -> Extraction {
        Extraction {
            file_path: PathBuf::from("f.py"),
            language: "Python".into(),
            nodes: vec![ExtractedNode {
                id: id.into(),
                label: label.into(),
                source_file: PathBuf::from("f.py"),
                source_line: Some(1),
                docstring: None,
                signature: None,
                node_type: "function".into(),
            }],
            edges: edges
                .into_iter()
                .map(|(s, t, rel, conf)| ExtractedEdge {
                    source: s.into(),
                    target: t.into(),
                    relation: rel.into(),
                    confidence: conf.into(),
                    confidence_score: Some(1.0),
                    source_file: PathBuf::from("f.py"),
                    source_line: Some(1),
                })
                .collect(),
        }
    }

    #[test]
    fn clean_extraction_passes() {
        assert_valid(&[ext("a", "a()", vec![("a", "b", "calls", "INFERRED")])]).unwrap();
    }

    #[test]
    fn dangling_endpoint_is_not_a_violation() {
        // Stubs for unresolved targets are by design.
        assert_valid(&[ext(
            "a",
            "a()",
            vec![("a", "missing_target", "calls", "INFERRED")],
        )])
        .unwrap();
    }

    #[test]
    fn empty_label_fails() {
        let err = assert_valid(&[ext("a", "", vec![])])
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty label"), "{err}");
    }

    #[test]
    fn empty_edge_endpoint_fails() {
        let err = assert_valid(&[ext("a", "a()", vec![("a", "", "calls", "INFERRED")])])
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty endpoint"), "{err}");
    }

    #[test]
    fn unknown_confidence_fails() {
        let err = assert_valid(&[ext("a", "a()", vec![("a", "b", "calls", "MAYBE")])])
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown confidence"), "{err}");
    }

    #[test]
    fn duplicate_node_id_fails() {
        let mut e = ext("a", "a()", vec![]);
        e.nodes.push(ExtractedNode {
            id: "a".into(),
            label: "a-again()".into(),
            source_file: PathBuf::from("f.py"),
            source_line: Some(2),
            docstring: None,
            signature: None,
            node_type: "function".into(),
        });
        let err = assert_valid(&[e]).unwrap_err().to_string();
        assert!(err.contains("duplicate node id"), "{err}");
    }

    #[test]
    fn all_issues_reported_not_first_only() {
        // An empty-id node and an endpoint-less edge each stop their own
        // record (can't cross-reference further), both still reported.
        let err = assert_valid(&[ext("", "", vec![("a", "", "calls", "MAYBE")])])
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty id"), "{err}");
        assert!(err.contains("empty endpoint"), "{err}");
        assert!(err.contains("(2 issues)"), "{err}");
    }
}
