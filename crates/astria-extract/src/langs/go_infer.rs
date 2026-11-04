//! Go type-aware post-pass, run over the whole corpus on every build.
//!
//! Three jobs, all name-derived evidence with fail-closed ambiguity:
//!
//! 1. `type X interface { M(...) }` declarations become class-kind nodes
//!    with their methods as members (the AST walker has no class handling
//!    for Go type declarations, so interfaces were invisible entirely).
//! 2. Implicit interface satisfaction: a type whose extracted method set
//!    is a superset of an interface's method names gets `dispatch` edges
//!    from each interface method to the implementing method — possible
//!    implementations, not proven calls, same as the TypeScript dispatch
//!    pass. Capped past [`MAX_IMPLEMENTERS`] types.
//! 3. Member calls on receiver-typed locals — `var s MemStore`,
//!    `s := MemStore{}`, `s := &MemStore{}`, `s := new(MemStore)` — resolve
//!    the `receiver::<caller>::<s>::<M>` call edge to the method's node.
//!    A receiver bound to two different types over the call's scope stays
//!    unresolved.

use crate::naming::{file_stem, make_node_id};
use crate::schema::{ExtractedEdge, ExtractedNode, Extraction};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

/// Past this many implementing types an interface is treated as saturated
/// and dispatch is skipped rather than fanning out across the graph.
const MAX_IMPLEMENTERS: usize = 10;

/// Statistics of one run, for tests and diagnostics.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GoInferStats {
    pub interfaces_added: usize,
    pub dispatch_added: usize,
    pub receivers_resolved: usize,
}

#[derive(Debug)]
struct GoInterface {
    name: String,
    line: u32,
    /// (method name, declaration line)
    methods: Vec<(String, u32)>,
}

#[derive(Debug)]
struct GoMethod {
    /// Receiver type, `*` and package qualifiers stripped.
    type_name: String,
    name: String,
    line: u32,
}

#[derive(Debug)]
struct GoBinding {
    var: String,
    type_name: String,
    /// Byte range where the binding is visible (enclosing function, or the
    /// whole file for package-level declarations).
    scope: Range<usize>,
}

#[derive(Debug)]
struct GoMemberCall {
    var: String,
    method: String,
    line: u32,
    position: usize,
}

/// Collected facts for one parsed Go file.
#[derive(Debug, Default)]
struct FileFacts {
    interfaces: Vec<GoInterface>,
    methods: Vec<GoMethod>,
    structs: Vec<String>,
    bindings: Vec<GoBinding>,
    calls: Vec<GoMemberCall>,
}

/// Run the pass. A no-op without the `lang-go` feature.
pub fn run(results: &mut [Extraction], root: &Path) -> GoInferStats {
    #[cfg(feature = "lang-go")]
    {
        run_with_parser(results, root)
    }
    #[cfg(not(feature = "lang-go"))]
    {
        let _ = (results, root);
        GoInferStats::default()
    }
}

#[cfg(feature = "lang-go")]
fn run_with_parser(results: &mut [Extraction], root: &Path) -> GoInferStats {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .is_err()
    {
        return GoInferStats::default();
    }
    let mut facts: HashMap<PathBuf, FileFacts> = HashMap::new();
    for ext in results.iter() {
        if ext.language != "Go" {
            continue;
        }
        let Ok(source) = std::fs::read(&ext.file_path) else {
            continue;
        };
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        let mut collected = FileFacts::default();
        collect_facts(
            tree.root_node(),
            &source,
            tree.root_node().byte_range(),
            &mut collected,
        );
        facts.insert(ext.file_path.clone(), collected);
    }
    if facts.is_empty() {
        return GoInferStats::default();
    }
    let mut stats = GoInferStats::default();
    add_interface_nodes(results, root, &facts, &mut stats);
    add_dispatch_edges(results, root, &facts, &mut stats);
    resolve_declared_receivers(results, &facts, &mut stats);
    stats
}

#[cfg(feature = "lang-go")]
fn text<'a>(node: tree_sitter::Node<'a>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

/// Strip `*`, package qualifiers, and generics from a type text:
/// `*pkg.MemStore` → "MemStore".
#[cfg(feature = "lang-go")]
fn bare_type_name(raw: &str) -> String {
    let trimmed = raw.trim().trim_start_matches('*');
    trimmed
        .rsplit('.')
        .next()
        .unwrap_or("")
        .split('[')
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

#[cfg(feature = "lang-go")]
fn each_child(node: tree_sitter::Node, mut visit: impl FnMut(tree_sitter::Node)) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child);
    }
}

#[cfg(feature = "lang-go")]
fn collect_facts(
    node: tree_sitter::Node,
    source: &[u8],
    scope: Range<usize>,
    facts: &mut FileFacts,
) {
    match node.kind() {
        "type_declaration" => {
            each_child(node, |spec| {
                if spec.kind() != "type_spec" {
                    return;
                }
                let Some(name_node) = spec.child_by_field_name("name") else {
                    return;
                };
                let name = text(name_node, source).to_string();
                let line = spec.start_position().row as u32 + 1;
                let Some(ty) = spec.child_by_field_name("type") else {
                    return;
                };
                match ty.kind() {
                    // tree-sitter-go names these interface_type/method_elem
                    // (older grammars said type_interface/method_spec).
                    "interface_type" | "type_interface" => {
                        let mut methods = Vec::new();
                        each_child(ty, |member| {
                            if !matches!(member.kind(), "method_elem" | "method_spec") {
                                return;
                            }
                            if let Some(mname) = member.child_by_field_name("name") {
                                methods.push((
                                    text(mname, source).to_string(),
                                    member.start_position().row as u32 + 1,
                                ));
                            }
                        });
                        facts.interfaces.push(GoInterface {
                            name,
                            line,
                            methods,
                        });
                    }
                    "type_struct" => facts.structs.push(name),
                    _ => {}
                }
            });
            each_child(node, |child| {
                collect_facts(child, source, scope.clone(), facts)
            });
        }
        "method_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                let receiver_type = node
                    .child_by_field_name("receiver")
                    .map(|receiver| {
                        let mut found = String::new();
                        each_child(receiver, |param| {
                            if param.kind() != "parameter_declaration" || !found.is_empty() {
                                return;
                            }
                            each_child(param, |part| {
                                if part.kind() == "identifier" {
                                    return;
                                }
                                let candidate = bare_type_name(text(part, source));
                                if !candidate.is_empty()
                                    && !candidate.chars().any(|c| c.is_whitespace())
                                {
                                    found.clone_from(&candidate);
                                }
                            });
                        });
                        found
                    })
                    .unwrap_or_default();
                let type_name = bare_type_name(&receiver_type);
                if !type_name.is_empty() {
                    facts.methods.push(GoMethod {
                        type_name,
                        name: text(name, source).to_string(),
                        line: node.start_position().row as u32 + 1,
                    });
                }
            }
            each_child(node, |child| {
                collect_facts(child, source, node.byte_range(), facts)
            });
        }
        "function_declaration" => {
            each_child(node, |child| {
                collect_facts(child, source, node.byte_range(), facts)
            });
        }
        "var_declaration" => {
            each_child(node, |spec| {
                if spec.kind() != "var_spec" {
                    return;
                }
                let type_name = spec
                    .child_by_field_name("type")
                    .map(|ty| bare_type_name(text(ty, source)))
                    .unwrap_or_default();
                if type_name.is_empty() || type_name.contains(char::is_whitespace) {
                    return;
                }
                if let Some(names) = spec.child_by_field_name("name") {
                    // The grammar emits a bare identifier for a single name
                    // and an expression_list for `var a, b T`.
                    let mut push = |name: tree_sitter::Node| {
                        facts.bindings.push(GoBinding {
                            var: text(name, source).to_string(),
                            type_name: type_name.clone(),
                            scope: scope.clone(),
                        });
                    };
                    match names.kind() {
                        "identifier" => push(names),
                        _ => each_child(names, |name| {
                            if name.kind() == "identifier" {
                                push(name);
                            }
                        }),
                    }
                }
            });
            each_child(node, |child| {
                collect_facts(child, source, scope.clone(), facts)
            });
        }
        "short_var_declaration" => {
            if let Some(binding) = short_var_binding(node, source) {
                let mut binding = binding;
                binding.scope = scope.start..scope.end.max(node.byte_range().end);
                facts.bindings.push(binding);
            }
            each_child(node, |child| {
                collect_facts(child, source, scope.clone(), facts)
            });
        }
        "call_expression" => {
            if let Some(function) = node.child_by_field_name("function") {
                if function.kind() == "selector_expression" {
                    if let (Some(object), Some(field)) = (
                        function.child_by_field_name("operand"),
                        function.child_by_field_name("field"),
                    ) {
                        if object.kind() == "identifier" {
                            facts.calls.push(GoMemberCall {
                                var: text(object, source).to_string(),
                                method: text(field, source).to_string(),
                                line: node.start_position().row as u32 + 1,
                                position: node.start_byte(),
                            });
                        }
                    }
                }
            }
            each_child(node, |child| {
                collect_facts(child, source, scope.clone(), facts)
            });
        }
        _ => each_child(node, |child| {
            collect_facts(child, source, scope.clone(), facts)
        }),
    }
}

/// `x := T{...}` / `x := &T{...}` / `x := pkg.T{...}` / `x := new(T)` —
/// only a single-name left side binds a type; tuples and function calls
/// do not.
#[cfg(feature = "lang-go")]
fn short_var_binding(node: tree_sitter::Node, source: &[u8]) -> Option<GoBinding> {
    let left = node.child_by_field_name("left")?;
    let right = node.child_by_field_name("value")?;
    let mut names: Vec<String> = Vec::new();
    each_child(left, |item| {
        if item.kind() == "identifier" {
            names.push(text(item, source).to_string());
        }
    });
    if names.len() != 1 {
        return None;
    }
    let type_name = value_type_name(right, source)?;
    Some(GoBinding {
        var: names.remove(0),
        type_name,
        scope: node.byte_range(),
    })
}

/// Type evidence from an initializer expression.
#[cfg(feature = "lang-go")]
fn value_type_name(value: tree_sitter::Node, source: &[u8]) -> Option<String> {
    match value.kind() {
        "composite_literal" => {
            let ty = value.child_by_field_name("type")?;
            let name = bare_type_name(text(ty, source));
            (!name.is_empty()).then_some(name)
        }
        "unary_expression" => value
            .child_by_field_name("operand")
            .and_then(|operand| value_type_name(operand, source)),
        "call_expression" => {
            let function = value.child_by_field_name("function")?;
            if text(function, source) == "new" {
                let mut walk = value.walk();
                let mut args = value.children(&mut walk);
                return args
                    .find(|a| a.kind() != "(" && a.kind() != ")")
                    .map(|a| bare_type_name(text(a, source)))
                    .filter(|n| !n.is_empty());
            }
            None
        }
        _ => None,
    }
}

/// Id stem for a Go file, identical to the walker's file ids.
#[cfg(feature = "lang-go")]
fn go_stem(file: &Path, root: &Path) -> String {
    let naming = file.strip_prefix(root).unwrap_or(file);
    file_stem(naming)
}

#[cfg(feature = "lang-go")]
fn add_interface_nodes(
    results: &mut [Extraction],
    root: &Path,
    facts: &HashMap<PathBuf, FileFacts>,
    stats: &mut GoInferStats,
) {
    for (file, collected) in facts {
        let Some(ext) = results.iter_mut().find(|e| &e.file_path == file) else {
            continue;
        };
        let stem = go_stem(file, root);
        let file_id = make_node_id(&[&stem]);
        for iface in &collected.interfaces {
            let iface_id = make_node_id(&[&stem, &iface.name]);
            if ext.nodes.iter().any(|n| n.id == iface_id) {
                continue;
            }
            ext.nodes.push(ExtractedNode {
                id: iface_id.clone(),
                label: iface.name.clone(),
                source_file: file.clone(),
                source_line: Some(iface.line),
                docstring: None,
                signature: None,
                node_type: "class".to_string(),
            });
            ext.edges.push(ExtractedEdge {
                source: file_id.clone(),
                target: iface_id.clone(),
                relation: "contains".to_string(),
                confidence: "EXTRACTED".to_string(),
                confidence_score: Some(1.0),
                source_file: file.clone(),
                source_line: Some(iface.line),
            });
            stats.interfaces_added += 1;
            for (method, line) in &iface.methods {
                let method_id = make_node_id(&[&stem, &iface.name, method]);
                if ext.nodes.iter().any(|n| n.id == method_id) {
                    continue;
                }
                ext.nodes.push(ExtractedNode {
                    id: method_id.clone(),
                    label: method.clone(),
                    source_file: file.clone(),
                    source_line: Some(*line),
                    docstring: None,
                    signature: None,
                    node_type: "method".to_string(),
                });
                ext.edges.push(ExtractedEdge {
                    source: iface_id.clone(),
                    target: method_id,
                    relation: "contains".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    confidence_score: Some(1.0),
                    source_file: file.clone(),
                    source_line: Some(*line),
                });
            }
        }
    }
}

/// Find the extracted node for a method declaration by (file, line), with
/// a unique-label fallback for declarations whose recorded line drifted.
/// Labels are compared with their `()` suffix stripped — the Go walker
/// labels functions `Get()` while declarations spell `Get`.
#[cfg(feature = "lang-go")]
fn method_node_id(results: &[Extraction], file: &Path, line: u32, label: &str) -> Option<String> {
    let ext = results.iter().find(|e| e.file_path == file)?;
    let matches_label = |n: &ExtractedNode| {
        n.label.trim_end_matches("()").eq_ignore_ascii_case(label)
            && n.node_type != "class"
            && n.node_type != "file"
    };
    let by_line: Vec<&ExtractedNode> = ext
        .nodes
        .iter()
        .filter(|n| n.source_line == Some(line) && matches_label(n))
        .collect();
    match by_line.as_slice() {
        [one] => Some(one.id.clone()),
        _ => {
            let by_label: Vec<&ExtractedNode> =
                ext.nodes.iter().filter(|n| matches_label(n)).collect();
            match by_label.as_slice() {
                [one] => Some(one.id.clone()),
                _ => None,
            }
        }
    }
}

#[cfg(feature = "lang-go")]
fn add_dispatch_edges(
    results: &mut [Extraction],
    root: &Path,
    facts: &HashMap<PathBuf, FileFacts>,
    stats: &mut GoInferStats,
) {
    // type name -> (method name -> (file, line)). The same method name on
    // the same type from two DIFFERENT directories is ambiguous (two
    // packages, two types); the same directory is one package — legal Go.
    let mut methods_by_type: HashMap<String, HashMap<String, (PathBuf, u32)>> = HashMap::new();
    let mut ambiguous_types: HashSet<String> = HashSet::new();
    for (file, collected) in facts {
        for method in &collected.methods {
            let entry = methods_by_type.entry(method.type_name.clone()).or_default();
            match entry.get(&method.name) {
                Some((existing, _)) if existing.parent() != file.parent() => {
                    ambiguous_types.insert(method.type_name.clone());
                }
                _ => {
                    entry.insert(method.name.clone(), (file.clone(), method.line));
                }
            }
        }
    }
    // Struct declarations give each type's home directory.
    let mut struct_dirs: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for (file, collected) in facts {
        for name in &collected.structs {
            struct_dirs
                .entry(name.clone())
                .or_default()
                .push(file.clone());
        }
    }
    let mut pending: Vec<(PathBuf, ExtractedEdge)> = Vec::new();
    for (file, collected) in facts {
        let iface_dir = file.parent();
        for iface in &collected.interfaces {
            let implementers: Vec<String> = methods_by_type
                .iter()
                .filter(|(type_name, methods)| {
                    type_name.as_str() != iface.name
                        && !ambiguous_types.contains(type_name.as_str())
                        && iface.methods.iter().all(|(m, _)| methods.contains_key(m))
                })
                .map(|(type_name, _)| type_name.clone())
                .collect();
            if implementers.is_empty() || implementers.len() > MAX_IMPLEMENTERS {
                continue;
            }
            // Same-package (same-directory) implementers first; the full
            // corpus-wide set is used only when no local type satisfies.
            let same_dir: Vec<&String> = implementers
                .iter()
                .filter(|t| {
                    struct_dirs
                        .get(t.as_str())
                        .is_some_and(|dirs| dirs.iter().any(|d| d.parent() == iface_dir))
                })
                .collect();
            let chosen: Vec<&String> = if same_dir.is_empty() {
                implementers.iter().collect()
            } else {
                same_dir
            };
            let stem = go_stem(file, root);
            for (method, line) in &iface.methods {
                let iface_method_id = make_node_id(&[&stem, &iface.name, method]);
                for type_name in chosen.iter() {
                    let Some((mfile, mline)) =
                        methods_by_type.get(*type_name).and_then(|m| m.get(method))
                    else {
                        continue;
                    };
                    let Some(target) = method_node_id(results, mfile, *mline, method) else {
                        continue;
                    };
                    pending.push((
                        file.clone(),
                        ExtractedEdge {
                            source: iface_method_id.clone(),
                            target,
                            relation: "dispatch".to_string(),
                            confidence: "INFERRED".to_string(),
                            // Possible implementations, not proven calls —
                            // the same tier as the TypeScript dispatch pass.
                            confidence_score: Some(0.6),
                            source_file: file.clone(),
                            source_line: Some(*line),
                        },
                    ));
                }
            }
        }
    }
    stats.dispatch_added = pending.len();
    for (file, edge) in pending {
        if let Some(ext) = results.iter_mut().find(|e| e.file_path == file) {
            ext.edges.push(edge);
        }
    }
}

#[cfg(feature = "lang-go")]
fn resolve_declared_receivers(
    results: &mut [Extraction],
    facts: &HashMap<PathBuf, FileFacts>,
    stats: &mut GoInferStats,
) {
    // (type, lowercased method) -> node id, from the collected method
    // declarations; a key seen with two different ids is dropped.
    let mut method_ids: HashMap<(String, String), String> = HashMap::new();
    let mut dropped_keys: HashSet<(String, String)> = HashSet::new();
    for (file, collected) in facts {
        for method in &collected.methods {
            let key = (method.type_name.clone(), method.name.to_ascii_lowercase());
            match method_node_id(results, file, method.line, &method.name) {
                Some(id) => match method_ids.get(&key) {
                    Some(existing) if existing != &id => {
                        dropped_keys.insert(key);
                    }
                    Some(_) => {}
                    None => {
                        method_ids.insert(key, id);
                    }
                },
                None => {
                    dropped_keys.insert(key);
                }
            }
        }
    }
    let mut rewrites: Vec<(usize, usize, String)> = Vec::new();
    for (file, collected) in facts {
        let Some(file_index) = results.iter().position(|e| e.file_path == *file) else {
            continue;
        };
        for (edge_index, edge) in results[file_index].edges.iter().enumerate() {
            if edge.relation != "calls" || !edge.target.starts_with("receiver::") {
                continue;
            }
            let Some(qualified) = edge
                .target
                .strip_prefix(&format!("receiver::{}::", edge.source))
            else {
                continue;
            };
            let mut parts = qualified.split("::");
            let (Some(var), Some(method), None) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            let Some(line) = edge.source_line else {
                continue;
            };
            // Every member call on this line must agree on exactly one
            // receiver type; otherwise the edge stays unresolved.
            let mut resolved_type: Option<String> = None;
            let mut ok = false;
            for call in collected.calls.iter().filter(|c| c.line == line) {
                if !call.var.eq_ignore_ascii_case(var) || !call.method.eq_ignore_ascii_case(method)
                {
                    continue;
                }
                let mut types: Vec<&str> = collected
                    .bindings
                    .iter()
                    .filter(|b| {
                        b.var.eq_ignore_ascii_case(&call.var) && b.scope.contains(&call.position)
                    })
                    .map(|b| b.type_name.as_str())
                    .filter(|t| !t.is_empty())
                    .collect();
                types.sort();
                types.dedup();
                if types.len() != 1 {
                    ok = false;
                    break;
                }
                let ty = types[0].to_string();
                match &resolved_type {
                    Some(previous) if previous != &ty => {
                        ok = false;
                    }
                    _ => {
                        resolved_type = Some(ty);
                        ok = true;
                    }
                }
                if !ok {
                    break;
                }
            }
            let Some(type_name) = resolved_type.filter(|_| ok) else {
                continue;
            };
            let key = (type_name, method.to_ascii_lowercase());
            if dropped_keys.contains(&key) {
                continue;
            }
            if let Some(target) = method_ids.get(&key) {
                rewrites.push((file_index, edge_index, target.clone()));
            }
        }
    }
    stats.receivers_resolved = rewrites.len();
    for (file_index, edge_index, target) in rewrites {
        let edge = &mut results[file_index].edges[edge_index];
        edge.target = target;
        edge.confidence = "RESOLVED".to_string();
        edge.confidence_score = Some(0.85);
    }
}
