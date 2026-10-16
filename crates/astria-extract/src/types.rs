//! Type-aware call resolution for the TypeScript family: receiver bindings
//! (constructor parameter properties, typed class fields, typed locals,
//! `new` initializers), class/interface heritage, and interface-dispatch
//! edges. Everything here is name/type-derived evidence — never
//! compiler-proven — so bindings resolve only through the same unique-name
//! rules as the rest of the resolver, and ambiguity stays unresolved.

use crate::schema::{ExtractedEdge, Extraction};
use std::collections::HashMap;
use std::path::PathBuf;

use tree_sitter::{Node, Parser};

#[derive(Debug, Clone)]
struct ReceiverBinding {
    file: PathBuf,
    /// Enclosing class label for fields and constructor parameter
    /// properties; None for locals declared outside any class.
    class_label: Option<String>,
    receiver: String,
    type_name: String,
}

#[derive(Debug, Clone)]
struct Heritage {
    file: PathBuf,
    class_label: String,
    /// (extends|implements, bare type name)
    bases: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
struct Declaration {
    file: PathBuf,
    label: String,
    kind: DeclKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeclKind {
    Class,
    Interface,
}

#[derive(Debug, Default)]
pub struct TypeIndex {
    bindings: Vec<ReceiverBinding>,
    heritage: Vec<Heritage>,
    declarations: Vec<Declaration>,
}

impl TypeIndex {
    /// Interface declarations (file, label).
    fn interfaces(&self) -> impl Iterator<Item = &Declaration> {
        self.declarations
            .iter()
            .filter(|d| d.kind == DeclKind::Interface)
    }
}

/// Bare type name from annotation or constructor text: strips type
/// arguments, namespaces, arrays, and whitespace.
fn bare_type_name(text: &str) -> String {
    let text = text.trim();
    let cut = text.find(['<', ' ', '[', '(']).unwrap_or(text.len());
    text[..cut]
        .rsplit(['.', '/'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Build the index by re-parsing the TypeScript-family files of the corpus
/// (.ts/.tsx/.mts/.cts). Parse failures contribute nothing — the resolver's
/// fail-closed rule governs everything downstream.
pub fn build_type_index(results: &[Extraction]) -> TypeIndex {
    let mut index = TypeIndex::default();
    for ext in results {
        let is_ts = matches!(ext.language.as_str(), "TypeScript" | "JavaScript")
            && matches!(
                ext.file_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .as_deref(),
                Some("ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs")
            );
        if !is_ts {
            continue;
        }
        let Ok(source) = std::fs::read(&ext.file_path) else {
            continue;
        };
        let tsx = matches!(
            ext.file_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .as_deref(),
            Some("tsx" | "jsx")
        );
        let grammar = if tsx {
            tree_sitter_typescript::LANGUAGE_TSX
        } else {
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT
        };
        let mut parser = Parser::new();
        if parser.set_language(&grammar.into()).is_err() {
            continue;
        }
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        let mut walker = Walker {
            source: &source,
            file: ext.file_path.clone(),
            class_stack: Vec::new(),
            index: &mut index,
        };
        walker.walk(tree.root_node());
    }
    index
}

struct Walker<'a> {
    source: &'a [u8],
    file: PathBuf,
    class_stack: Vec<String>,
    index: &'a mut TypeIndex,
}

impl<'a> Walker<'a> {
    fn text(&self, node: Node) -> &'a str {
        node.utf8_text(self.source).unwrap_or("")
    }

    fn walk(&mut self, node: Node) {
        match node.kind() {
            "class_declaration" | "class" => {
                let label = node
                    .child_by_field_name("name")
                    .map(|n| self.text(n).to_string());
                if let Some(label) = label {
                    self.index.declarations.push(Declaration {
                        file: self.file.clone(),
                        label: label.clone(),
                        kind: DeclKind::Class,
                    });
                    self.collect_heritage(node, &label);
                    self.class_stack.push(label);
                    self.walk_children(node);
                    self.class_stack.pop();
                    return;
                }
                self.walk_children(node);
            }
            "interface_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.index.declarations.push(Declaration {
                        file: self.file.clone(),
                        label: self.text(name).to_string(),
                        kind: DeclKind::Interface,
                    });
                }
                self.walk_children(node);
            }
            "property_definition"
            | "public_field_definition"
            | "property_signature"
            | "property_declaration" => {
                if let (Some(class), Some(name)) =
                    (self.class_stack.last(), node.child_by_field_name("name"))
                {
                    let receiver = self.text(name).to_string();
                    // Annotation wins over initializer: `svc: S = new T()`
                    // declares S.
                    let mut anno_cursor = node.walk();
                    let annotation = node
                        .named_children(&mut anno_cursor)
                        .find(|c| c.kind().contains("type_annotation"));
                    let type_name = annotation
                        .map(|ty| bare_type_name(self.text(ty).trim_start_matches(':')))
                        .filter(|t| !t.is_empty())
                        .or_else(|| {
                            node.child_by_field_name("value")
                                .and_then(|value| new_expression_type(value, self.source))
                        });
                    if let Some(type_name) = type_name {
                        self.index.bindings.push(ReceiverBinding {
                            file: self.file.clone(),
                            class_label: Some(class.clone()),
                            receiver,
                            type_name,
                        });
                    }
                }
                self.walk_children(node);
            }
            "method_definition" | "method_signature" => {
                let is_constructor = node
                    .child_by_field_name("name")
                    .is_some_and(|n| self.text(n) == "constructor");
                if is_constructor {
                    self.collect_parameter_properties(node);
                }
                self.walk_children(node);
            }
            "lexical_declaration" | "variable_declaration" => {
                let mut cursor = node.walk();
                for declarator in node.named_children(&mut cursor) {
                    if declarator.kind() != "variable_declarator" {
                        continue;
                    }
                    let Some(name) = declarator.child_by_field_name("name") else {
                        continue;
                    };
                    let receiver = self.text(name).to_string();
                    let mut type_name = declarator
                        .child_by_field_name("type")
                        .map(|ty| bare_type_name(self.text(ty)))
                        .unwrap_or_default();
                    if type_name.is_empty() {
                        if let Some(value) = declarator.child_by_field_name("value") {
                            type_name = new_expression_type(value, self.source).unwrap_or_default();
                        }
                    }
                    if !type_name.is_empty() {
                        self.index.bindings.push(ReceiverBinding {
                            file: self.file.clone(),
                            class_label: self.class_stack.last().cloned(),
                            receiver,
                            type_name,
                        });
                    }
                }
                self.walk_children(node);
            }
            _ => self.walk_children(node),
        }
    }

    fn walk_children(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.walk(child);
        }
    }

    /// `constructor(private svc: Service, readonly repo: Repo)` — TS
    /// parameter properties bind a receiver per parameter.
    fn collect_parameter_properties(&mut self, method: Node) {
        let Some(params) = method.child_by_field_name("parameters") else {
            return;
        };
        let mut cursor = params.walk();
        for param in params.named_children(&mut cursor) {
            if !matches!(param.kind(), "required_parameter" | "optional_parameter") {
                continue;
            }
            let has_modifier = {
                let mut inner = param.walk();
                let found = param.children(&mut inner).any(|c| {
                    matches!(
                        c.kind(),
                        "accessibility_modifier" | "readonly" | "override_modifier"
                    )
                });
                found
            };
            if !has_modifier {
                continue;
            }
            let Some(pattern) = param.child_by_field_name("pattern") else {
                continue;
            };
            let receiver = self.text(pattern).trim_start_matches("this.").to_string();
            let mut anno_cursor = param.walk();
            let annotation = param
                .named_children(&mut anno_cursor)
                .find(|c| c.kind().contains("type_annotation"));
            if let Some(ty) = annotation {
                let type_name = bare_type_name(self.text(ty).trim_start_matches(':'));
                if !type_name.is_empty() {
                    self.index.bindings.push(ReceiverBinding {
                        file: self.file.clone(),
                        class_label: self.class_stack.last().cloned(),
                        receiver,
                        type_name,
                    });
                }
            }
        }
    }

    fn collect_heritage(&mut self, class_node: Node, label: &str) {
        let mut bases = Vec::new();
        let mut cursor = class_node.walk();
        for child in class_node.children(&mut cursor) {
            if !matches!(child.kind(), "class_heritage") {
                continue;
            }
            // The heritage node wraps clause nodes (extends_clause /
            // implements_clause) whose own children carry the keyword and
            // the type names.
            let mut clauses = child.walk();
            for clause in child.children(&mut clauses) {
                let clause_text = self.text(clause).trim_start();
                let clause_kind = if clause.kind().starts_with("implements")
                    || clause_text.starts_with("implements")
                {
                    "implements"
                } else {
                    "extends"
                };
                let mut tokens = clause.walk();
                for token in clause.children(&mut tokens) {
                    let text = self.text(token);
                    if matches!(text, "extends" | "implements" | "," | "(" | ")" | "{" | "") {
                        continue;
                    }
                    let name = bare_type_name(text);
                    if name.is_empty() || !name.chars().next().is_some_and(|c| c.is_uppercase()) {
                        continue;
                    }
                    bases.push((clause_kind.to_string(), name));
                }
            }
        }
        if !bases.is_empty() {
            self.index.heritage.push(Heritage {
                file: self.file.clone(),
                class_label: label.to_string(),
                bases,
            });
        }
    }
}

/// `new Service(...)` → "Service".
fn new_expression_type(value: Node, source: &[u8]) -> Option<String> {
    if value.kind() != "new_expression" {
        return None;
    }
    let constructor = value.child_by_field_name("constructor")?;
    let name = bare_type_name(constructor.utf8_text(source).unwrap_or(""));
    (!name.is_empty()).then_some(name)
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// Per-file class/interface member index used by both resolution passes.
struct ClassView {
    /// (file-key, label) -> class-like node ids.
    decl_ids: HashMap<(String, String), Vec<String>>,
    /// class-like node id -> (member label -> member node id).
    members: HashMap<String, HashMap<String, String>>,
}

fn file_key(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn build_class_view(results: &[Extraction]) -> ClassView {
    let mut decl_ids: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut members: HashMap<String, HashMap<String, String>> = HashMap::new();
    for ext in results {
        if !matches!(ext.language.as_str(), "TypeScript" | "JavaScript") {
            continue;
        }
        let file = file_key(&ext.file_path);
        let class_ids: Vec<String> = ext
            .nodes
            .iter()
            .filter(|n| n.node_type == "class")
            .map(|n| {
                decl_ids
                    .entry((file.clone(), n.label.clone()))
                    .or_default()
                    .push(n.id.clone());
                n.id.clone()
            })
            .collect();
        // Membership comes from the walker's `contains` edges: member node
        // ids are not string-prefixes of their class id, so prefix matching
        // would silently lose every member.
        let label_by_id: HashMap<&str, &str> = ext
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.label.as_str()))
            .collect();
        for edge in &ext.edges {
            if edge.relation != "contains" {
                continue;
            }
            let Some(label) = label_by_id.get(edge.target.as_str()) else {
                continue;
            };
            let tail = label.trim_end_matches("()").to_string();
            if tail.is_empty() || !class_ids.contains(&edge.source) {
                continue;
            }
            members
                .entry(edge.source.clone())
                .or_default()
                .insert(tail, edge.target.clone());
        }
    }
    ClassView { decl_ids, members }
}

impl ClassView {
    /// Resolve a type name to class-like node ids: same file first, then a
    /// unique same-named declaration anywhere in the corpus.
    fn resolve_type(&self, file: &str, type_name: &str) -> Vec<String> {
        if let Some(ids) = self
            .decl_ids
            .get(&(file.to_string(), type_name.to_string()))
        {
            return ids.clone();
        }
        let all: Vec<String> = self
            .decl_ids
            .iter()
            .filter(|((_, label), _)| label == type_name)
            .flat_map(|(_, ids)| ids.iter().cloned())
            .collect();
        if all.len() == 1 {
            all
        } else {
            Vec::new()
        }
    }

    /// Member lookup through `extends` heritage (transitive, cycle-guarded).
    fn lookup_member(
        &self,
        index: &TypeIndex,
        class_ids: &[String],
        member: &str,
        depth: usize,
    ) -> Option<String> {
        if depth > 6 {
            return None;
        }
        for id in class_ids {
            if let Some(member_id) = self.members.get(id).and_then(|m| m.get(member)) {
                return Some(member_id.clone());
            }
        }
        for heritage in &index.heritage {
            let class_labels: Vec<&String> = class_ids
                .iter()
                .filter_map(|id| {
                    self.decl_ids
                        .iter()
                        .find(|(_, ids)| ids.contains(id))
                        .map(|((_, label), _)| label)
                })
                .collect();
            if !class_labels.iter().any(|l| **l == heritage.class_label) {
                continue;
            }
            let bases: Vec<String> = heritage
                .bases
                .iter()
                .filter(|(kind, _)| kind == "extends")
                .filter_map(|(_, name)| {
                    let ids = self
                        .decl_ids
                        .get(&(file_key(&heritage.file), name.clone()))
                        .cloned()
                        .unwrap_or_default();
                    (!ids.is_empty()).then_some(ids).or_else(|| {
                        let anywhere: Vec<String> = self
                            .decl_ids
                            .iter()
                            .filter(|((_, label), _)| label == name)
                            .flat_map(|(_, ids)| ids.iter().cloned())
                            .collect();
                        (anywhere.len() == 1).then_some(anywhere)
                    })
                })
                .flatten()
                .collect();
            if !bases.is_empty() {
                if let Some(found) = self.lookup_member(index, &bases, member, depth + 1) {
                    return Some(found);
                }
            }
        }
        None
    }
}

/// Outcome of typed-receiver resolution for one edge.
pub struct TypedCalls {
    /// (file index, edge index, member node id) — resolved bindings.
    pub rewrites: Vec<(usize, usize, String)>,
    /// (file index, edge index, `receiver::type::<Type>::<member>`) —
    /// unresolved here but with retained type evidence for cross-repo
    /// matching.
    pub typed_unresolved: Vec<(usize, usize, String)>,
}

/// Resolve `receiver::<caller>::<recv>.<member>` call edges through the
/// declared type of the receiver. TS family only; ambiguity stays
/// untouched.
pub fn resolve_typed_receivers(results: &[Extraction], index: &TypeIndex) -> TypedCalls {
    let view = build_class_view(results);
    let mut rewrites = Vec::new();
    let mut typed_unresolved = Vec::new();

    for (file_index, ext) in results.iter().enumerate() {
        if !matches!(ext.language.as_str(), "TypeScript" | "JavaScript") {
            continue;
        }
        let file = file_key(&ext.file_path);
        // The caller's enclosing class: `contains` edges are the walker's
        // own membership record (ids flatten labels, so prefix matching on
        // class ids does not work).
        let class_label_by_id: HashMap<&str, &str> = ext
            .nodes
            .iter()
            .filter(|n| n.node_type == "class")
            .map(|n| (n.id.as_str(), n.label.as_str()))
            .collect();
        let mut class_of_member: HashMap<&str, &str> = HashMap::new();
        for edge in &ext.edges {
            if edge.relation == "contains" && class_label_by_id.contains_key(edge.source.as_str()) {
                class_of_member.insert(edge.target.as_str(), edge.source.as_str());
            }
        }
        for (edge_index, edge) in ext.edges.iter().enumerate() {
            if edge.relation != "calls" || !edge.target.starts_with("receiver::") {
                continue;
            }
            let Some(qualified) = edge
                .target
                .strip_prefix(&format!("receiver::{}::", edge.source))
            else {
                continue;
            };
            // make_target_id flattened the dotted expression to '::'.
            let normalized = qualified.replace("::", ".");
            let Some((receiver, method)) = normalized.rsplit_once('.') else {
                continue;
            };
            let receiver = receiver.trim_start_matches("this.");
            let caller_class = class_of_member
                .get(edge.source.as_str())
                .and_then(|class_id| class_label_by_id.get(class_id))
                .map(|label| label.to_string());
            let Some(type_name) = index
                .bindings
                .iter()
                .filter(|b| file_key(&b.file) == file)
                .filter(|b| match (&b.class_label, &caller_class) {
                    (Some(binding_class), Some(caller)) => binding_class == caller,
                    (None, None) => true,
                    // A method body sees its class fields; a binding
                    // recorded inside the class matches any member.
                    (Some(_), None) => false,
                    (None, Some(_)) => false,
                })
                .find(|b| b.receiver == receiver)
                .map(|b| b.type_name.clone())
            else {
                continue;
            };

            let candidates = view.resolve_type(&file, &type_name);
            if candidates.is_empty() {
                typed_unresolved.push((
                    file_index,
                    edge_index,
                    format!("receiver::type::{type_name}::{method}"),
                ));
                continue;
            }
            match view.lookup_member(index, &candidates, method, 0) {
                Some(member_id) => rewrites.push((file_index, edge_index, member_id)),
                None => typed_unresolved.push((
                    file_index,
                    edge_index,
                    format!("receiver::type::{type_name}::{method}"),
                )),
            }
        }
    }
    TypedCalls {
        rewrites,
        typed_unresolved,
    }
}

/// Interface dispatch edges: connect every interface member to the
/// same-named member of each class that implements the interface. Possible
/// implementations, not proven runtime calls — INFERRED confidence with
/// the `dispatch` relation so call-flow and impact views label them as
/// such.
pub fn add_dispatch_edges(results: &mut [Extraction], index: &TypeIndex) -> usize {
    let view = build_class_view(results);
    // Interface label -> ids (from AST declarations, not node kinds, so
    // plain classes never masquerade as interfaces).
    let mut interface_ids: HashMap<String, Vec<String>> = HashMap::new();
    for decl in index.interfaces() {
        let ids = view
            .decl_ids
            .get(&(file_key(&decl.file), decl.label.clone()))
            .cloned()
            .unwrap_or_default();
        interface_ids
            .entry(decl.label.clone())
            .or_default()
            .extend(ids);
    }
    let mut added = 0usize;
    for heritage in &index.heritage {
        let implemented: Vec<&String> = heritage
            .bases
            .iter()
            .filter(|(kind, _)| kind == "implements")
            .map(|(_, name)| name)
            .collect();
        if implemented.is_empty() {
            continue;
        }
        let file = file_key(&heritage.file);
        let Some(implementer_ids) = view
            .decl_ids
            .get(&(file.clone(), heritage.class_label.clone()))
        else {
            continue;
        };
        for interface_name in implemented {
            let Some(interface_candidates) = interface_ids.get(interface_name.as_str()) else {
                continue;
            };
            // Interfaces are declared once per project in practice; where
            // several share a name the ambiguity stays unlinked rather
            // than guessing an implementer relationship.
            if interface_candidates.len() != 1 {
                continue;
            }
            let interface_id = interface_candidates[0].clone();
            let interface_members = view.members.get(&interface_id);
            let Some(interface_members) = interface_members else {
                continue;
            };
            for implementer_id in implementer_ids {
                let Some(implementer_members) = view.members.get(implementer_id) else {
                    continue;
                };
                for (member_label, interface_member_id) in interface_members {
                    let Some(impl_member_id) = implementer_members.get(member_label) else {
                        continue;
                    };
                    let Some(ext) = results.iter_mut().find(|e| e.file_path == heritage.file)
                    else {
                        continue;
                    };
                    ext.edges.push(ExtractedEdge {
                        source: interface_member_id.clone(),
                        target: impl_member_id.clone(),
                        relation: "dispatch".to_string(),
                        confidence: "INFERRED".to_string(),
                        // Possible implementations, not proven calls: scored
                        // at the SEMANTIC tier (below the 0.65 weak floor) so
                        // dispatch-only reach cannot outrank directly
                        // matched files in query output.
                        confidence_score: Some(0.6),
                        source_file: heritage.file.clone(),
                        source_line: None,
                    });
                    added += 1;
                }
            }
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ExtractedEdge, ExtractedNode, ExtractionStatus};

    fn extraction(file: &std::path::Path, source_lang: &str, text: &str) -> Extraction {
        std::fs::write(file, text).unwrap();
        let cfg = crate::langs::get_language_for_extension(
            file.extension().and_then(|e| e.to_str()).unwrap_or("ts"),
        )
        .unwrap();
        let mut ext = crate::walkers::extract_source(file, cfg, file, text.as_bytes()).unwrap();
        ext.language = source_lang.to_string();
        ext.status = ExtractionStatus::Complete;
        ext
    }

    #[test]
    fn typed_receiver_resolves_through_declared_field_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("svc.ts");
        let source = "class Store {\n  save(): void {}\n}\nclass App {\n  store: Store;\n  run(): void {\n    this.store.save();\n  }\n}\n";
        let mut results = vec![extraction(&path, "TypeScript", source)];
        let index = build_type_index(&results);
        // Walker produced a receiver:: edge for this.store.save().
        assert!(
            results[0]
                .edges
                .iter()
                .any(|e| e.target.starts_with("receiver::") && e.target.contains("store::save")),
            "walker must retain the receiver call: {:?}",
            results[0].edges
        );
        let typed = resolve_typed_receivers(&results, &index);
        assert_eq!(
            typed.rewrites.len(),
            1,
            "this.store.save() resolves via the field type"
        );
        let (file_idx, edge_idx, target) = &typed.rewrites[0];
        assert!(results[*file_idx].edges[*edge_idx].target.contains("save"));
        assert!(target.contains("save"));
    }

    #[test]
    fn constructor_parameter_properties_bind_receivers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("di.ts");
        let source = "class Service {\n  go(): void {}\n}\nclass Client {\n  constructor(private service: Service) {}\n  use(): void {\n    this.service.go();\n  }\n}\n";
        let results = vec![extraction(&path, "TypeScript", source)];
        let index = build_type_index(&results);
        assert!(
            index
                .bindings
                .iter()
                .any(|b| b.receiver == "service" && b.type_name == "Service"),
            "constructor parameter property recorded: {:?}",
            index.bindings
        );
    }

    #[test]
    fn dispatch_edges_link_interface_members_to_implementers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("iface.ts");
        let source = "interface Sender {\n  send(msg: string): void;\n}\nclass EmailSender implements Sender {\n  send(msg: string): void {}\n  log(): void {}\n}\n";
        let mut results = vec![extraction(&path, "TypeScript", source)];
        let index = build_type_index(&results);
        let added = add_dispatch_edges(&mut results, &index);
        assert!(
            added >= 1,
            "interface member send() dispatches to EmailSender.send()"
        );
        let dispatch: Vec<&ExtractedEdge> = results[0]
            .edges
            .iter()
            .filter(|e| e.relation == "dispatch")
            .collect();
        assert!(dispatch
            .iter()
            .all(|e| e.confidence == "INFERRED" && e.source.contains("send")));
        assert!(dispatch.iter().any(|e| e.target.contains("send")));
    }

    #[test]
    fn bare_type_name_strips_generics_and_namespaces() {
        assert_eq!(bare_type_name("Service<X>"), "Service");
        assert_eq!(bare_type_name("ns.Service[]"), "Service");
    }
}
