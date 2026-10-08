//! Typed records describe exactly the complete records delivered on a page.
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryNode {
    pub id: String,
    pub label: String,
    pub file_type: String,
    pub source_file: String,
    pub source_line: Option<i64>,
    pub community: Option<i64>,
    pub signature: Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub confidence: String,
    pub confidence_score: Option<f64>,
    pub source_file: String,
    pub source_line: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResponse {
    pub text: String,
    /// Total matching subgraph, distinct from records delivered on this page.
    pub node_count: usize,
    pub edge_count: usize,
    pub next_cursor: Option<usize>,
    pub nodes: Vec<QueryNode>,
    pub edges: Vec<QueryEdge>,
    pub graph_built_at: Option<String>,
    pub graph_generation: Option<String>,
    pub freshness: Option<astria_detect::freshness::GraphFreshness>,
    /// The budget applies to rendered text; JSON transport overhead is separate.
    pub rendered_tokens: usize,
    pub elapsed_milliseconds: u64,
    pub snapshot_estimated_bytes: usize,
}

impl QueryResponse {
    pub(crate) fn into_output(self) -> super::QueryOutput {
        (
            self.text,
            self.node_count,
            self.edge_count,
            self.next_cursor,
        )
    }
}
