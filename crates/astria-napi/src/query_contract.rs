//! Native bindings for the shared query and freshness contracts.
use napi_derive::napi;

#[napi(object)]
pub struct GraphFreshnessJs {
    pub status: String,
    pub added: i64,
    pub modified: i64,
    pub deleted: i64,
    pub files_checked: i64,
    pub extraction_outdated: bool,
    pub artifacts_checked: bool,
    pub artifacts_consistent: Option<bool>,
    pub graph_generation: Option<String>,
    pub graph_built_at: Option<String>,
    pub stale_external_indexes: Vec<String>,
    pub check_milliseconds: i64,
    pub error: Option<String>,
}
impl From<astria_detect::freshness::GraphFreshness> for GraphFreshnessJs {
    fn from(f: astria_detect::freshness::GraphFreshness) -> Self {
        Self { status: f.status, added: f.added as i64, modified: f.modified as i64,
            deleted: f.deleted as i64, files_checked: f.files_checked as i64,
            extraction_outdated: f.extraction_outdated, artifacts_checked: f.artifacts_checked,
            artifacts_consistent: f.artifacts_consistent, graph_generation: f.graph_generation,
            graph_built_at: f.graph_built_at, stale_external_indexes: f.stale_external_indexes,
            check_milliseconds: f.check_milliseconds as i64, error: f.error }
    }
}

#[napi]
pub fn graph_freshness(root: String) -> napi::Result<GraphFreshnessJs> {
    let root = std::path::PathBuf::from(root).canonicalize()?;
    let db = crate::pipeline::load_graph_db(&root)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let transaction = db.unchecked_transaction()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let result = astria_detect::freshness::inspect(&db, &root, true);
    transaction.commit().map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(result.into())
}

#[napi(object)]
pub struct QueryNodeJs {
    pub id: String, pub label: String, pub file_type: String,
    pub source_file: String, pub source_line: Option<i64>, pub community: Option<i64>,
    pub signature: Option<String>, pub summary: Option<String>,
}
#[napi(object)]
pub struct QueryEdgeJs {
    pub source: String, pub target: String, pub relation: String, pub confidence: String,
    pub confidence_score: Option<f64>, pub source_file: String, pub source_line: Option<i64>,
}
#[napi(object)]
pub struct QueryResultJs {
    pub text: String, pub node_count: i64, pub edge_count: i64, pub next_cursor: Option<i64>,
    pub nodes: Vec<QueryNodeJs>, pub edges: Vec<QueryEdgeJs>,
    pub graph_built_at: Option<String>, pub graph_generation: Option<String>,
    pub freshness: Option<GraphFreshnessJs>, pub rendered_tokens: i64,
    pub elapsed_milliseconds: i64, pub snapshot_estimated_bytes: i64,
}
impl From<astria_query::QueryResponse> for QueryResultJs {
    fn from(r: astria_query::QueryResponse) -> Self {
        Self {
            text: r.text, node_count: r.node_count as i64, edge_count: r.edge_count as i64,
            next_cursor: r.next_cursor.map(|n| n as i64),
            nodes: r.nodes.into_iter().map(|n| QueryNodeJs {
                id: n.id, label: n.label, file_type: n.file_type, source_file: n.source_file,
                source_line: n.source_line, community: n.community, signature: n.signature, summary: n.summary,
            }).collect(),
            edges: r.edges.into_iter().map(|e| QueryEdgeJs {
                source: e.source, target: e.target, relation: e.relation, confidence: e.confidence,
                confidence_score: e.confidence_score, source_file: e.source_file, source_line: e.source_line,
            }).collect(),
            graph_built_at: r.graph_built_at, graph_generation: r.graph_generation,
            freshness: r.freshness.map(Into::into), rendered_tokens: r.rendered_tokens as i64,
            elapsed_milliseconds: r.elapsed_milliseconds as i64,
            snapshot_estimated_bytes: r.snapshot_estimated_bytes as i64,
        }
    }
}
