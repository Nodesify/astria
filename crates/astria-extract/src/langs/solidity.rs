// Solidity language config for the shared tree-sitter walker.

use super::config::LanguageConfig;

pub fn config() -> &'static LanguageConfig {
    static CONFIG: LanguageConfig = LanguageConfig {
        name: astria_core::languages::LanguageId::Solidity
            .registration()
            .name,
        extensions: astria_core::languages::LanguageId::Solidity
            .registration()
            .extensions,
        #[cfg(feature = "lang-solidity")]
        language_fn: || tree_sitter_solidity::LANGUAGE.into(),
        #[cfg(not(feature = "lang-solidity"))]
        language_fn: || crate::langs::config::missing_language("solidity"),
        compiled_in: cfg!(feature = "lang-solidity"),
        class_types: &[
            "contract_declaration",
            "interface_declaration",
            "library_declaration",
            "struct_declaration",
            "enum_declaration",
        ],
        function_types: &[
            "function_definition",
            "modifier_definition",
            "constructor_definition",
        ],
        import_types: &["import_directive", "using_directive"],
        call_type: "call_expression",
        name_child: Some(1),
        name_field: "name",
        body_field: Some("body"),
        body_fallback_types: &[],
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

    fn extract_solidity(source: &str) -> crate::schema::Extraction {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("token.sol");
        fs::write(&file, source).unwrap();
        let db = astria_core::db::open_db_in_memory().unwrap();
        let mut results = engine::extract(&[file], dir.path(), &db).unwrap();
        std::mem::forget(dir);
        assert_eq!(results.len(), 1);
        results.remove(0)
    }

    #[test]
    fn solidity_contracts_functions_and_imports() {
        let ext = extract_solidity("import \"./IERC20.sol\";\n\ncontract Token {\n  function transfer(address to) public {\n    _send(to);\n  }\n}\n");
        assert_eq!(ext.language, "Solidity");
        let labels: Vec<&str> = ext.nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(labels.contains(&"Token"), "contract: {labels:?}");
        assert!(labels.contains(&"transfer()"), "function: {labels:?}");
        assert!(ext.edges.iter().any(|e| e.relation == "imports"));
        assert!(ext.edges.iter().any(|e| e.relation == "contains"));
    }
}
