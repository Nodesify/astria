use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
}

/// Copy a fixture directory into a temp directory so tests can run in parallel
/// without interfering with each other's .astria state.
fn copy_fixture_to_temp(subdir: &str) -> tempfile::TempDir {
    let src = fixtures_dir().join(subdir);
    let tmp = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(&src).unwrap() {
        let entry = entry.unwrap();
        let dst = tmp.path().join(entry.file_name());
        std::fs::copy(entry.path(), dst).unwrap();
    }
    tmp
}

#[test]
fn full_pipeline_on_python_fixture() {
    let tmp = copy_fixture_to_temp("python");
    let root = tmp.path();
    let result = astria_napi::pipeline::run_pipeline(root).unwrap();

    assert!(
        result.build_result.nodes_added > 0,
        "should extract nodes from Python fixture"
    );
    assert!(
        result.build_result.edges_added > 0,
        "should extract edges from Python fixture"
    );
    assert!(
        !result.analysis.god_nodes.is_empty(),
        "should find god nodes"
    );
    assert!(result.report.contains("# Graph Report"));

    assert!(root.join(".astria/db.sqlite").exists());
    assert!(root.join(".astria/graph_report.md").exists());
    assert!(root.join(".astria/graph.json").exists());
}

#[test]
fn full_pipeline_on_rust_fixture() {
    let tmp = copy_fixture_to_temp("rust");
    let root = tmp.path();
    let result = astria_napi::pipeline::run_pipeline(root).unwrap();
    assert!(result.build_result.nodes_added > 0);
}

#[test]
fn full_pipeline_on_javascript_fixture() {
    let tmp = copy_fixture_to_temp("javascript");
    let root = tmp.path();
    let result = astria_napi::pipeline::run_pipeline(root).unwrap();
    assert!(result.build_result.nodes_added > 0);
}

#[test]
fn incremental_update_adds_no_duplicate_nodes() {
    let tmp = copy_fixture_to_temp("python");
    let root = tmp.path();
    let r1 = astria_napi::pipeline::run_pipeline(root).unwrap();
    assert!(r1.build_result.nodes_added > 0);

    let r2 = astria_napi::pipeline::run_pipeline(root).unwrap();
    assert_eq!(
        r2.build_result.nodes_added, 0,
        "second run on unchanged files should add 0 nodes"
    );
}

#[test]
fn export_json_is_valid() {
    let tmp = copy_fixture_to_temp("typescript");
    let root = tmp.path();
    let _ = astria_napi::pipeline::run_pipeline(root).unwrap();

    let json_str = std::fs::read_to_string(root.join(".astria/graph.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert!(parsed["nodes"].is_array());
    assert!(parsed["edges"].is_array());
    assert!(!parsed["nodes"].as_array().unwrap().is_empty());
}

#[test]
fn load_graph_db_errors_on_missing_graph_without_creating_it() {
    let tmp = tempfile::tempdir().unwrap();
    let err = astria_napi::pipeline::load_graph_db(tmp.path())
        .expect_err("loading a missing graph should be an error");
    assert!(
        err.to_string().contains("No graph found"),
        "error should tell the user to run the pipeline first, got: {err}"
    );
    assert!(
        !tmp.path().join(".astria").exists(),
        "read-only load must not create the .astria directory"
    );
}

#[test]
fn mcp_server_refuses_missing_graph_without_creating_it() {
    let tmp = tempfile::tempdir().unwrap();
    let err = astria_napi::run_mcp_server(tmp.path().to_string_lossy().to_string())
        .expect_err("MCP server on a missing graph should fail fast");
    assert!(
        err.to_string().contains("No graph found"),
        "error should tell the user to build the graph first, got: {err}"
    );
    assert!(
        !tmp.path().join(".astria").exists(),
        "MCP server must not create the .astria directory"
    );
}
