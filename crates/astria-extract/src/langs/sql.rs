// SQL language config for the shared tree-sitter walker.
// Object names follow the two leading keywords (CREATE TABLE <name>), hence name_child 3 (1-based named children).

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Sql.registration().name,
        extensions: astria_core::languages::LanguageId::Sql
            .registration()
            .extensions,
        #[cfg(feature = "lang-sql")]
        language_fn: || tree_sitter_sequel::LANGUAGE.into(),
        #[cfg(not(feature = "lang-sql"))]
        language_fn: || crate::langs::config::missing_language("sql"),
        compiled_in: cfg!(feature = "lang-sql"),
        class_types: &[
            "create_table",
            "create_view",
            "create_materialized_view",
            "create_schema",
            "create_database",
        ],
        function_types: &["create_function", "create_trigger", "create_query"],
        import_types: &[],
        call_type: "invocation",
        name_child: Some(3),
        name_field: "",
        body_field: None,
        body_fallback_types: &["function_body"],
        class_call_names: &[],
        function_call_names: &[],
        import_call_names: &[],
        closure_types: &[],
    };
    &CONFIG
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine;
    use std::fs;

    fn extract_sql(source: &str) -> crate::schema::Extraction {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("schema.sql");
        fs::write(&file, source).unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    #[test]
    fn sql_tables_views_functions_and_triggers() {
        let ext = extract_sql("CREATE TABLE users (id INT, name TEXT);\nCREATE VIEW adults AS SELECT * FROM users;\nCREATE FUNCTION add_one(i INT) RETURNS INT AS $$ BEGIN RETURN i + 1; END $$ LANGUAGE plpgsql;\nCREATE TRIGGER t_after AFTER INSERT ON users EXECUTE FUNCTION add_one(1);\n");
        assert_eq!(ext.language, "SQL");
        let joined: String = ext
            .nodes
            .iter()
            .map(|n| n.label.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert!(joined.contains("users"), "table: {joined}");
        assert!(joined.contains("adults"), "view: {joined}");
        assert!(joined.contains("add_one"), "function: {joined}");
        assert!(joined.contains("t_after"), "trigger: {joined}");
    }
}
