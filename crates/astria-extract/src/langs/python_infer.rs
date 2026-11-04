//! Python type-aware post-pass: module-level singletons resolve their
//! member calls.
//!
//! `g = Service()` at module level, then `g.method()` anywhere in the same
//! file, rewrites the `receiver::<caller>::<g>::<method>` call edge to the
//! class's method node (RESOLVED tier, same as the TypeScript receiver
//! machinery). Fail-closed guards, mirroring the upstream heuristic this
//! ports: the binding is dropped when the name is ever rebound (assigned,
//! augmented, or looped) anywhere in the file, and a call is skipped when
//! the name is shadowed by a parameter or local assignment inside its
//! enclosing function.

use crate::schema::Extraction;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// Statistics of one run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PythonInferStats {
    pub receivers_resolved: usize,
}

#[derive(Debug)]
struct MemberCall {
    var: String,
    method: String,
    line: u32,
    position: usize,
}

/// Facts for one parsed Python file.
#[derive(Debug, Default)]
struct FileFacts {
    /// var -> class name, module-level singleton bindings that survive the
    /// rebinding guard.
    singletons: HashMap<String, String>,
    /// var -> byte ranges where the name is shadowed (function scopes with
    /// a parameter or local assignment of that name).
    shadowed: HashMap<String, Vec<Range<usize>>>,
    calls: Vec<MemberCall>,
}

/// Run the pass. A no-op without the `lang-python` feature or when no
/// Python extractions exist.
pub fn run(results: &mut [Extraction]) -> PythonInferStats {
    #[cfg(feature = "lang-python")]
    {
        run_with_parser(results)
    }
    #[cfg(not(feature = "lang-python"))]
    {
        let _ = results;
        PythonInferStats::default()
    }
}

#[cfg(feature = "lang-python")]
fn text<'a>(node: tree_sitter::Node<'a>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

#[cfg(feature = "lang-python")]
fn each_child(node: tree_sitter::Node, mut visit: impl FnMut(tree_sitter::Node)) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child);
    }
}

#[cfg(feature = "lang-python")]
fn run_with_parser(results: &mut [Extraction]) -> PythonInferStats {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .is_err()
    {
        return PythonInferStats::default();
    }
    let mut facts: HashMap<std::path::PathBuf, FileFacts> = HashMap::new();
    for ext in results.iter() {
        if ext.language != "Python" {
            continue;
        }
        let Ok(source) = std::fs::read(&ext.file_path) else {
            continue;
        };
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        let mut collected = FileFacts::default();
        collect_facts(tree.root_node(), &source, &mut collected);
        facts.insert(ext.file_path.clone(), collected);
    }
    if facts.is_empty() {
        return PythonInferStats::default();
    }

    // Class label -> node id, unique across the corpus (an ambiguous class
    // name never resolves a singleton).
    let mut class_ids: HashMap<&str, &str> = HashMap::new();
    let mut dropped_classes: HashSet<&str> = HashSet::new();
    for ext in results.iter() {
        for node in &ext.nodes {
            if node.node_type != "class" {
                continue;
            }
            match class_ids.get(node.label.as_str()) {
                Some(existing) if *existing != node.id => {
                    dropped_classes.insert(node.label.as_str());
                }
                Some(_) => {}
                None => {
                    class_ids.insert(node.label.as_str(), node.id.as_str());
                }
            }
        }
    }
    // class node id -> (member label, lowercased) -> member node id,
    // unique per class; ambiguous member labels are dropped.
    let mut members: HashMap<&str, HashMap<String, &str>> = HashMap::new();
    for ext in results.iter() {
        let label_by_id: HashMap<&str, &str> = ext
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.label.as_str()))
            .collect();
        for edge in &ext.edges {
            if edge.relation != "contains" {
                continue;
            }
            let (Some(&label), Some(target_label)) = (
                label_by_id.get(edge.source.as_str()),
                label_by_id.get(edge.target.as_str()),
            ) else {
                continue;
            };
            if !dropped_classes.contains(label)
                && class_ids.get(label) == Some(&edge.source.as_str())
            {
                let entry = members.entry(edge.source.as_str()).or_default();
                let key = target_label.trim_end_matches("()").to_ascii_lowercase();
                match entry.get(&key) {
                    Some(existing) if *existing != edge.target.as_str() => {
                        entry.remove(&key);
                    }
                    Some(_) => {}
                    None => {
                        entry.insert(key, edge.target.as_str());
                    }
                }
            }
        }
    }

    let mut rewrites: Vec<(usize, usize, String)> = Vec::new();
    for (file, collected) in &facts {
        let Some(file_index) = results.iter().position(|e| &e.file_path == file) else {
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
            // Every member call on this line must agree on one singleton
            // binding; anything else leaves the edge unresolved.
            let mut class_name: Option<&String> = None;
            let mut ok = false;
            for call in collected.calls.iter().filter(|c| c.line == line) {
                if !call.var.eq_ignore_ascii_case(var) || !call.method.eq_ignore_ascii_case(method)
                {
                    continue;
                }
                if collected
                    .shadowed
                    .get(&call.var)
                    .is_some_and(|ranges| ranges.iter().any(|r| r.contains(&call.position)))
                {
                    ok = false;
                    break;
                }
                match collected.singletons.get(&call.var) {
                    Some(found) => match class_name {
                        Some(previous) if previous != found => {
                            ok = false;
                        }
                        _ => {
                            class_name = Some(found);
                            ok = true;
                        }
                    },
                    None => {
                        ok = false;
                    }
                }
                if !ok {
                    break;
                }
            }
            let Some(class) = class_name.filter(|_| ok) else {
                continue;
            };
            if dropped_classes.contains(class.as_str()) {
                continue;
            }
            let Some(class_id) = class_ids.get(class.as_str()) else {
                continue;
            };
            if let Some(target) = members
                .get(class_id)
                .and_then(|m| m.get(&method.to_ascii_lowercase()))
            {
                rewrites.push((file_index, edge_index, (*target).to_string()));
            }
        }
    }
    let stats = PythonInferStats {
        receivers_resolved: rewrites.len(),
    };
    for (file_index, edge_index, target) in rewrites {
        let edge = &mut results[file_index].edges[edge_index];
        edge.target = target;
        edge.confidence = "RESOLVED".to_string();
        edge.confidence_score = Some(0.85);
    }
    stats
}

/// Walk one Python file, tracking the module scope and each function's
/// shadowing ranges.
#[cfg(feature = "lang-python")]
fn collect_facts(node: tree_sitter::Node, source: &[u8], facts: &mut FileFacts) {
    match node.kind() {
        "module" => {
            // Module-level assignments. Statements wrap them in
            // `expression_statement`, so look one level down. The FIRST
            // bare-constructor assignment binds; any later write of the
            // same name at module level is a rebinding and drops it.
            let mut bound: HashSet<String> = HashSet::new();
            each_child(node, |child| {
                // Accept both shapes: bare `assignment` and the
                // `expression_statement` wrapper tree-sitter-python emits.
                let assignment = match child.kind() {
                    "assignment" => child,
                    "expression_statement" => {
                        let mut walk = child.walk();
                        let found = child
                            .named_children(&mut walk)
                            .find(|c| c.kind() == "assignment");
                        match found {
                            Some(assignment) => assignment,
                            None => return,
                        }
                    }
                    _ => return,
                };
                let Some(left) = assignment.child_by_field_name("left") else {
                    return;
                };
                if left.kind() != "identifier" {
                    return;
                }
                let var = text(left, source).to_string();
                if bound.contains(&var) {
                    facts.singletons.remove(&var);
                    return;
                }
                bound.insert(var.clone());
                let Some(right) = assignment.child_by_field_name("right") else {
                    return;
                };
                if right.kind() == "call" {
                    if let Some(function) = right.child_by_field_name("function") {
                        // A bare identifier call is a constructor spelling;
                        // attribute calls (mod.Service()) carry no local
                        // class binding.
                        if function.kind() == "identifier" {
                            facts
                                .singletons
                                .insert(var, text(function, source).to_string());
                        }
                    }
                }
            });
            each_child(node, |child| collect_facts(child, source, facts));
        }
        "function_definition" => {
            // Parameters and any local assignment shadow a module name.
            let range = node.byte_range();
            let mut names: Vec<String> = Vec::new();
            if let Some(params) = node.child_by_field_name("parameters") {
                each_child(params, |param| {
                    collect_param_names(param, source, &mut names);
                });
            }
            collect_shadow_assignments(node, source, &mut names);
            for name in names {
                facts.shadowed.entry(name).or_default().push(range.clone());
            }
            each_child(node, |child| collect_facts(child, source, facts));
        }
        "call" => {
            if let Some(function) = node.child_by_field_name("function") {
                if function.kind() == "attribute" {
                    if let (Some(object), Some(attr)) = (
                        function.child_by_field_name("object"),
                        function.child_by_field_name("attribute"),
                    ) {
                        if object.kind() == "identifier" {
                            facts.calls.push(MemberCall {
                                var: text(object, source).to_string(),
                                method: text(attr, source).to_string(),
                                line: node.start_position().row as u32 + 1,
                                position: node.start_byte(),
                            });
                        }
                    }
                }
            }
            each_child(node, |child| collect_facts(child, source, facts));
        }
        "assignment" | "augmented_assignment" | "for_statement" => {
            // Rebinding kills the module binding — except the module-level
            // creators, which the `module` branch already accounted for
            // (through their expression_statement wrapper, if any).
            let at_module_level = node.parent().is_some_and(|parent| {
                parent.kind() == "module"
                    || (parent.kind() == "expression_statement"
                        && parent
                            .parent()
                            .is_some_and(|grandparent| grandparent.kind() == "module"))
            });
            if !at_module_level {
                if let Some(left) = node.child_by_field_name("left") {
                    let mut names: Vec<String> = Vec::new();
                    collect_target_names(left, source, &mut names);
                    for name in names {
                        facts.singletons.remove(&name);
                    }
                }
            }
            each_child(node, |child| collect_facts(child, source, facts));
        }
        _ => each_child(node, |child| collect_facts(child, source, facts)),
    }
}

#[cfg(feature = "lang-python")]
fn collect_param_names(param: tree_sitter::Node, source: &[u8], names: &mut Vec<String>) {
    match param.kind() {
        "identifier" => names.push(text(param, source).to_string()),
        _ => each_child(param, |child| collect_param_names(child, source, names)),
    }
}

#[cfg(feature = "lang-python")]
fn collect_target_names(target: tree_sitter::Node, source: &[u8], names: &mut Vec<String>) {
    match target.kind() {
        "identifier" => names.push(text(target, source).to_string()),
        _ => each_child(target, |child| collect_target_names(child, source, names)),
    }
}

/// Names assigned anywhere inside a function body (locals), for the
/// shadow guard.
#[cfg(feature = "lang-python")]
fn collect_shadow_assignments(node: tree_sitter::Node, source: &[u8], names: &mut Vec<String>) {
    match node.kind() {
        "assignment" | "augmented_assignment" | "for_statement" => {
            if let Some(left) = node.child_by_field_name("left") {
                collect_target_names(left, source, names);
            }
            each_child(node, |child| {
                collect_shadow_assignments(child, source, names)
            });
        }
        _ => each_child(node, |child| {
            collect_shadow_assignments(child, source, names)
        }),
    }
}
