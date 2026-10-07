//! Standard SCIP ingestion. `Extraction.file_path` owns the index; node and
//! edge source paths are document citations, independent of update ownership.
use astria_core::{AstriaError, Result};
use astria_extract::{ExtractedEdge, ExtractedNode, Extraction};
use protobuf::Message;
use scip::types::{occurrence::Typed_range, Index, Occurrence, SymbolInformation};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

fn error(path: &Path, message: impl Into<String>) -> AstriaError {
    AstriaError::Parse {
        file: path.display().to_string(),
        message: message.into(),
    }
}

fn symbol_id(document: &Path, symbol: &str) -> String {
    let mut hash = Sha256::new();
    // Global SCIP symbols already contain package/version identity. Only
    // local symbols require document scope.
    if scip::symbol::is_local_symbol(symbol) {
        hash.update(document.to_string_lossy().replace('\\', "/").as_bytes());
        hash.update([0]);
    }
    hash.update(symbol.as_bytes());
    format!("scip_{:x}", hash.finalize())
}

fn validate_symbol(path: &Path, symbol: &str) -> Result<()> {
    if scip::symbol::is_local_symbol(symbol) {
        scip::symbol::try_parse_local_symbol(symbol)
            .map_err(|e| error(path, format!("invalid SCIP symbol: {e:?}")))?;
    } else {
        scip::symbol::parse_symbol(symbol)
            .map_err(|e| error(path, format!("invalid SCIP symbol: {e:?}")))?;
    }
    Ok(())
}

fn occurrence_line(path: &Path, occurrence: &Occurrence) -> Result<u32> {
    let (line, start, end_line, end) = match &occurrence.typed_range {
        Some(Typed_range::SingleLineRange(r)) => {
            (r.line, r.start_character, r.line, r.end_character)
        }
        Some(Typed_range::MultiLineRange(r)) => {
            (r.start_line, r.start_character, r.end_line, r.end_character)
        }
        None => match occurrence.range.as_slice() {
            [line, start, end] => (*line, *start, *line, *end),
            [line, start, end_line, end] => (*line, *start, *end_line, *end),
            _ => {
                return Err(error(
                    path,
                    "invalid SCIP occurrence range: expected 3 or 4 elements",
                ))
            }
        },
        _ => return Err(error(path, "unsupported SCIP occurrence range")),
    };
    if line < 0 || start < 0 || end_line < line || end < 0 || (line == end_line && end < start) {
        return Err(error(path, "invalid SCIP occurrence range bounds"));
    }
    Ok(line as u32 + 1)
}

fn edge(
    source: String,
    target: String,
    relation: &str,
    path: &Path,
    line: Option<u32>,
) -> ExtractedEdge {
    ExtractedEdge {
        source,
        target,
        relation: relation.into(),
        confidence: "EXTRACTED".into(),
        confidence_score: Some(1.0),
        source_file: path.to_path_buf(),
        source_line: line,
    }
}

fn symbol_node(
    id: String,
    symbol: &str,
    info: Option<&SymbolInformation>,
    definition: Option<&(PathBuf, u32)>,
) -> ExtractedNode {
    use scip::types::symbol_information::Kind;
    ExtractedNode {
        id,
        label: info
            .filter(|i| !i.display_name.is_empty())
            .map(|i| i.display_name.clone())
            .unwrap_or_else(|| symbol.to_string()),
        // External definitions without an occurrence have no invented citation.
        source_file: definition.map(|(p, _)| p.clone()).unwrap_or_default(),
        source_line: definition.map(|(_, l)| *l),
        docstring: info
            .filter(|i| !i.documentation.is_empty())
            .map(|i| i.documentation.join("\n\n")),
        signature: info
            .and_then(|i| i.signature_documentation.as_ref())
            .filter(|s| !s.text.is_empty())
            .map(|s| s.text.clone()),
        node_type: info
            .map(|i| match i.kind.enum_value_or_default() {
                Kind::Function | Kind::Method | Kind::StaticMethod => "function",
                Kind::Class | Kind::Struct | Kind::Interface | Kind::Trait => "class",
                _ => "code",
            })
            .unwrap_or("reference")
            .into(),
    }
}

/// Parse native SCIP protobuf or standard protobuf JSON (no custom JSON shape).
pub fn parse_scip(scip_path: &Path, bytes: impl AsRef<[u8]>) -> Result<Extraction> {
    let bytes = bytes.as_ref();
    let index: Index = if bytes.iter().copied().find(|b| !b.is_ascii_whitespace()) == Some(b'{') {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| error(scip_path, format!("invalid SCIP JSON encoding: {e}")))?;
        protobuf_json_mapping::parse_from_str(text)
            .map_err(|e| error(scip_path, format!("invalid SCIP protobuf JSON: {e}")))?
    } else {
        Index::parse_from_bytes(bytes)
            .map_err(|e| error(scip_path, format!("invalid SCIP protobuf: {e}")))?
    };
    let metadata = index
        .metadata
        .as_ref()
        .ok_or_else(|| error(scip_path, "missing SCIP metadata"))?;
    let root = url::Url::parse(&metadata.project_root)
        .map_err(|e| error(scip_path, format!("invalid SCIP project_root URI: {e}")))?
        .to_file_path()
        .map_err(|_| error(scip_path, "SCIP project_root must be an absolute file URI"))?;
    if !root.is_absolute() {
        return Err(error(scip_path, "SCIP project_root must be absolute"));
    }
    let mut documents = Vec::new();
    let mut definitions = HashMap::new();
    let mut seen_paths = HashSet::new();
    for document in &index.documents {
        let relative = &document.relative_path;
        if relative.is_empty()
            || relative.contains(['\\', ':'])
            || relative.split('/').any(|p| matches!(p, "" | "." | ".."))
        {
            return Err(error(
                scip_path,
                format!("invalid SCIP relative_path: {relative}"),
            ));
        }
        if !seen_paths.insert(relative) {
            return Err(error(
                scip_path,
                format!("duplicate SCIP document: {relative}"),
            ));
        }
        let path = root.join(relative);
        let file_id = format!(
            "scip_document_{:x}",
            Sha256::digest(path.to_string_lossy().replace('\\', "/").as_bytes())
        );
        for occurrence in &document.occurrences {
            let line = occurrence_line(scip_path, occurrence)?;
            if occurrence.symbol.is_empty() {
                continue;
            }
            validate_symbol(scip_path, &occurrence.symbol)?;
            if occurrence.symbol_roles & 1 != 0 {
                definitions
                    .entry(symbol_id(&path, &occurrence.symbol))
                    .or_insert((path.clone(), line));
            }
        }
        documents.push((document, path, file_id));
    }
    let mut nodes = BTreeMap::new();
    let mut edges = Vec::new();
    for (document, path, file_id) in &documents {
        nodes.insert(
            file_id.clone(),
            ExtractedNode {
                id: file_id.clone(),
                label: document.relative_path.clone(),
                source_file: path.clone(),
                source_line: None,
                docstring: None,
                signature: None,
                node_type: "file".into(),
            },
        );
        for info in &document.symbols {
            validate_symbol(scip_path, &info.symbol)?;
            let id = symbol_id(path, &info.symbol);
            nodes.insert(
                id.clone(),
                symbol_node(id.clone(), &info.symbol, Some(info), definitions.get(&id)),
            );
            for relationship in &info.relationships {
                validate_symbol(scip_path, &relationship.symbol)?;
                let target = symbol_id(path, &relationship.symbol);
                for (flag, relation) in [
                    (relationship.is_implementation, "scip_impl"),
                    (relationship.is_type_definition, "scip_typed"),
                    (relationship.is_reference, "scip_ref"),
                    (relationship.is_definition, "scip_def"),
                ] {
                    if flag {
                        edges.push(edge(
                            id.clone(),
                            target.clone(),
                            relation,
                            path,
                            definitions
                                .get(&id)
                                .filter(|(p, _)| p == path)
                                .map(|(_, l)| *l),
                        ));
                    }
                }
                nodes.entry(target.clone()).or_insert_with(|| {
                    symbol_node(
                        target.clone(),
                        &relationship.symbol,
                        None,
                        definitions.get(&target),
                    )
                });
            }
        }
        for occurrence in &document.occurrences {
            if occurrence.symbol.is_empty() {
                continue;
            }
            let id = symbol_id(path, &occurrence.symbol);
            nodes.entry(id.clone()).or_insert_with(|| {
                symbol_node(id.clone(), &occurrence.symbol, None, definitions.get(&id))
            });
            edges.push(edge(
                file_id.clone(),
                id,
                if occurrence.symbol_roles & 1 != 0 {
                    "scip_def"
                } else {
                    "scip_ref"
                },
                path,
                Some(occurrence_line(scip_path, occurrence)?),
            ));
        }
    }
    for info in &index.external_symbols {
        if scip::symbol::is_local_symbol(&info.symbol) {
            return Err(error(scip_path, "external SCIP symbol must be global"));
        }
        validate_symbol(scip_path, &info.symbol)?;
        let id = symbol_id(&root, &info.symbol);
        let external = symbol_node(id.clone(), &info.symbol, Some(info), definitions.get(&id));
        if nodes.get(&id).is_none_or(|n| n.node_type == "reference") {
            nodes.insert(id.clone(), external);
        }
        for relationship in &info.relationships {
            if scip::symbol::is_local_symbol(&relationship.symbol) {
                return Err(error(
                    scip_path,
                    "external SCIP relationship must target a global symbol",
                ));
            }
            validate_symbol(scip_path, &relationship.symbol)?;
            let target = symbol_id(&root, &relationship.symbol);
            let (source_path, line) = definitions
                .get(&id)
                .map(|(p, l)| (p.as_path(), Some(*l)))
                .unwrap_or((Path::new(""), None));
            for (flag, relation) in [
                (relationship.is_implementation, "scip_impl"),
                (relationship.is_type_definition, "scip_typed"),
                (relationship.is_reference, "scip_ref"),
                (relationship.is_definition, "scip_def"),
            ] {
                if flag {
                    edges.push(edge(
                        id.clone(),
                        target.clone(),
                        relation,
                        source_path,
                        line,
                    ));
                }
            }
            nodes.entry(target.clone()).or_insert_with(|| {
                symbol_node(
                    target.clone(),
                    &relationship.symbol,
                    None,
                    definitions.get(&target),
                )
            });
        }
    }
    if documents.is_empty() {
        return Err(error(scip_path, "SCIP index has no documents"));
    }
    Ok(Extraction {
        file_path: scip_path.to_path_buf(),
        language: "SCIP".into(),
        nodes: nodes.into_values().collect(),
        edges,
    })
}

pub fn parse_scip_file(scip_path: &Path) -> Result<Extraction> {
    // Ownership must identify the index itself, independent of whether callers
    // spell its path relatively, absolutely, or through a filesystem alias.
    let canonical = scip_path
        .canonicalize()
        .map_err(|e| error(scip_path, format!("resolve index path failed: {e}")))?;
    let bytes =
        std::fs::read(&canonical).map_err(|e| error(scip_path, format!("read failed: {e}")))?;
    parse_scip(&canonical, bytes)
}
