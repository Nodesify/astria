// Deterministic manifest ingestion: pyproject.toml, Cargo.toml, go.mod,
// package.json, pom.xml → one canonical `pkg_<name>` node per package plus
// `depends_on` edges. Ported from upstream astria v8 manifest_ingest.py /
// cargo_introspect.py. Never uses an LLM — dependency structure is fact, not
// inference, and a shared dependency should emerge as a hub node.

use crate::schema::ExtractionStatus;
use std::path::Path;

use astria_core::ids::normalize_id;
use astria_core::MANIFEST_FILENAMES;

use crate::schema::{ExtractedEdge, ExtractedNode, Extraction};

pub fn is_manifest(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| MANIFEST_FILENAMES.contains(&n.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn pkg_id(name: &str) -> String {
    format!("pkg_{}", normalize_id(name))
}

/// Read `package.name` from a Cargo.toml file, if it parses.
fn read_crate_name(cargo_toml: &Path) -> Option<String> {
    std::fs::read_to_string(cargo_toml)
        .ok()?
        .parse::<toml::Table>()
        .ok()?
        .get("package")?
        .get("name")?
        .as_str()
        .map(|s| s.to_string())
}

/// Bounded upward walk (3 levels) from `start` to find the workspace root's
/// `[workspace.dependencies].<name>` path entry — how `{ workspace = true }`
/// inherited deps resolve to a crate directory. The entry's path is manifest
/// content: it resolves only when it stays inside `root`.
fn find_workspace_dep_dir(dep_name: &str, start: &Path, root: &Path) -> Option<std::path::PathBuf> {
    let mut dir = start.parent();
    for _ in 0..3 {
        let d = dir?;
        let ws_toml = d.join("Cargo.toml");
        if let Ok(text) = std::fs::read_to_string(&ws_toml) {
            if let Ok(doc) = text.parse::<toml::Table>() {
                if let Some(rel) = doc
                    .get("workspace")
                    .and_then(|w| w.get("dependencies"))
                    .and_then(|deps| deps.get(dep_name))
                    .and_then(|v| v.as_table())
                    .and_then(|t| t.get("path"))
                    .and_then(|p| p.as_str())
                {
                    return astria_core::security::confine_to_root(d, rel, root);
                }
            }
        }
        dir = d.parent();
    }
    None
}

struct ManifestBuilder {
    file_path: std::path::PathBuf,
    nodes: Vec<ExtractedNode>,
    edges: Vec<ExtractedEdge>,
}

impl ManifestBuilder {
    fn new(path: &Path) -> Self {
        Self {
            file_path: path.to_path_buf(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    fn add_package(&mut self, name: &str, docstring: Option<&str>) -> String {
        let id = pkg_id(name);
        if !self.nodes.iter().any(|n| n.id == id) {
            self.nodes.push(ExtractedNode {
                id: id.clone(),
                label: name.to_string(),
                source_file: self.file_path.clone(),
                source_line: Some(1),
                docstring: docstring.map(|s| s.to_string()),
                signature: None,
                node_type: "package".to_string(),
            });
        }
        id
    }

    fn add_dependency(&mut self, from: &str, dep_name: &str) {
        let dep_id = self.add_package(dep_name, None);
        if from != dep_id {
            self.edges.push(ExtractedEdge {
                source: from.to_string(),
                target: dep_id,
                relation: "depends_on".to_string(),
                confidence: "EXTRACTED".to_string(),
                confidence_score: Some(1.0),
                source_file: self.file_path.clone(),
                source_line: Some(1),
            });
        }
    }
}

/// Ingest one manifest file. `root` is the scanned tree the file was found
/// in: paths named INSIDE the manifest (workspace members, `path`
/// dependencies) are followed only when they stay within it.
pub fn extract_manifest(path: &Path, root: &Path) -> Extraction {
    let mut builder = ManifestBuilder::new(path);
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    let text = std::fs::read_to_string(path).unwrap_or_default();

    match filename.as_str() {
        "pyproject.toml" => ingest_pyproject(&text, &mut builder),
        "cargo.toml" => ingest_cargo(path, root, &text, &mut builder),
        "go.mod" => ingest_go_mod(&text, &mut builder),
        "package.json" => ingest_package_json(&text, &mut builder),
        "pom.xml" => ingest_pom(&text, &mut builder),
        ".mcp.json" | "mcp.json" | "mcp_servers.json" | "claude_desktop_config.json" => {
            ingest_mcp_config(path, &text, &mut builder)
        }
        _ => {}
    }

    Extraction {
        file_path: path.to_path_buf(),
        language: "Manifest".to_string(),
        nodes: builder.nodes,
        edges: builder.edges,
        status: ExtractionStatus::Complete,
        notes: Vec::new(),
    }
}

fn ingest_pyproject(text: &str, b: &mut ManifestBuilder) {
    let Ok(doc) = text.parse::<toml::Table>() else {
        return;
    };
    let Some(project) = doc.get("project").and_then(|v| v.as_table()) else {
        return;
    };
    let name = project
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown-python-project");
    let self_id = b.add_package(name, Some("this project (pyproject.toml)"));
    if let Some(deps) = project.get("dependencies").and_then(|v| v.as_array()) {
        for dep in deps.iter().filter_map(|d| d.as_str()) {
            // PEP 508: take the name before the first version marker
            let dep_name = dep
                .split(|c: char| {
                    c == '='
                        || c == '<'
                        || c == '>'
                        || c == '~'
                        || c == '!'
                        || c == ';'
                        || c == '['
                        || c.is_whitespace()
                })
                .next()
                .unwrap_or("")
                .trim();
            if !dep_name.is_empty() {
                b.add_dependency(&self_id, dep_name);
            }
        }
    }
}

fn ingest_cargo(path: &Path, root: &Path, text: &str, b: &mut ManifestBuilder) {
    let Ok(doc) = text.parse::<toml::Table>() else {
        return;
    };

    if let Some(package) = doc.get("package").and_then(|v| v.as_table()) {
        let name = package
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown-crate");
        let self_id = b.add_package(name, Some("this crate (Cargo.toml)"));
        // NOTE: [dependencies] and friends are TOP-LEVEL tables in Cargo.toml,
        // not nested under [package] — the original lookup inside `package`
        // silently captured nothing.
        let base = path.parent().unwrap_or(path);
        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(deps) = doc.get(section).and_then(|v| v.as_table()) {
                for (dep_name, entry) in deps {
                    b.add_dependency(&self_id, dep_name);
                    // Internal path deps: link crate -> crate when the path
                    // stays inside the workspace and the target's own
                    // Cargo.toml names it. `{ workspace = true }` inherits the
                    // real entry from the workspace root's
                    // [workspace.dependencies] (found by a bounded upward walk).
                    let dep_dir: Option<std::path::PathBuf> = match entry {
                        // The path is manifest content: it is followed only
                        // when it stays inside the scanned tree.
                        toml::Value::Table(t) => {
                            t.get("path").and_then(|p| p.as_str()).and_then(|rel| {
                                astria_core::security::confine_to_root(base, rel, root)
                            })
                        }
                        _ => None,
                    }
                    .or_else(|| {
                        let inherited = entry
                            .as_table()
                            .and_then(|t| t.get("workspace"))
                            .and_then(|w| w.as_bool())
                            == Some(true);
                        if inherited {
                            find_workspace_dep_dir(dep_name, base, root)
                        } else {
                            None
                        }
                    });
                    if let Some(dir) = dep_dir {
                        let target_toml = dir.join("Cargo.toml");
                        if let Some(crate_name) = read_crate_name(&target_toml) {
                            let dep_id = pkg_id(&crate_name);
                            if dep_id != self_id {
                                b.edges.push(ExtractedEdge {
                                    source: self_id.clone(),
                                    target: dep_id,
                                    relation: "crate_depends_on".to_string(),
                                    confidence: "EXTRACTED".to_string(),
                                    confidence_score: Some(1.0),
                                    source_file: b.file_path.clone(),
                                    source_line: Some(1),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Workspace: one node per member crate, linked with contains so the
    // internal crate graph emerges. Member names are read from each member's
    // own Cargo.toml when resolvable.
    if let Some(workspace) = doc.get("workspace").and_then(|v| v.as_table()) {
        let root_id = b.add_package(
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("workspace"),
            Some("cargo workspace"),
        );
        if let Some(members) = workspace.get("members").and_then(|v| v.as_array()) {
            for member in members.iter().filter_map(|m| m.as_str()) {
                let dir = member.trim_end_matches("/*").trim_end_matches('/');
                // A member path is manifest content: its Cargo.toml is read
                // only when it stays inside the scanned tree; anything else
                // keeps the directory-derived name.
                let crate_name = astria_core::security::confine_to_root(
                    path.parent().unwrap_or(path),
                    dir,
                    root,
                )
                .map(|member_dir| member_dir.join("Cargo.toml"))
                .and_then(|member_toml| std::fs::read_to_string(&member_toml).ok())
                .and_then(|t| t.parse::<toml::Table>().ok())
                .and_then(|d| {
                    d.get("package")
                        .and_then(|p| p.as_table())
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| dir.replace('/', "_"));
                let member_id = b.add_package(&crate_name, Some("workspace member"));
                if member_id != root_id {
                    b.edges.push(ExtractedEdge {
                        source: root_id.clone(),
                        target: member_id,
                        relation: "contains".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        confidence_score: Some(1.0),
                        source_file: b.file_path.clone(),
                        source_line: Some(1),
                    });
                }
            }
        }
    }
}

/// MCP server configs (`.mcp.json` & friends): servers, commands, packages,
/// and required env var NAMES. Values (commands aside) are never read —
/// secrets in `env` blocks must not leak into the graph.
fn ingest_mcp_config(path: &Path, text: &str, b: &mut ManifestBuilder) {
    const MAX_MCP_CONFIG_BYTES: usize = 1024 * 1024;
    if text.len() > MAX_MCP_CONFIG_BYTES {
        return;
    }
    let Ok(doc) = text.parse::<serde_json::Value>() else {
        return;
    };
    let Some(servers) = doc.get("mcpServers").and_then(|v| v.as_object()) else {
        return;
    };
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mcp")
        .to_lowercase();
    let config_id = format!("mcp_config_{}", normalize_id(&stem));
    b.nodes.push(ExtractedNode {
        id: config_id.clone(),
        label: path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("mcp.json")
            .to_string(),
        source_file: b.file_path.clone(),
        source_line: None,
        docstring: Some("MCP server configuration".to_string()),
        signature: None,
        node_type: "mcp_config".to_string(),
    });

    let package_re = |arg: &str| -> Option<(String, String)> {
        // Package reference among the args: "@scope/name@1.2", "name@^2",
        // "name/path" (npm), or a python module ("some_pkg", passed via -m).
        if arg.starts_with('-') || arg.is_empty() {
            return None; // flags aren't packages
        }
        let name = if arg.starts_with('@') {
            // npm scoped: @scope/name[@version]
            let core = arg.trim_start_matches('@');
            core.split('@').next().unwrap_or("")
        } else {
            arg.split('@').next().unwrap_or(arg)
        };
        if name.is_empty() {
            return None;
        }
        if name.contains('/') || name.contains('@') || arg.starts_with('@') || name.contains('_') {
            Some((format!("mcp_pkg_{}", normalize_id(name)), name.to_string()))
        } else {
            None
        }
    };

    for (name, cfg) in servers {
        let server_id = format!("mcp_server_{}::{}", normalize_id(&stem), normalize_id(name));
        b.nodes.push(ExtractedNode {
            id: server_id.clone(),
            label: format!("mcp:{name}"),
            source_file: b.file_path.clone(),
            source_line: Some(1),
            docstring: cfg
                .get("command")
                .and_then(|c| c.as_str())
                .map(|c| format!("MCP server launched via `{c}`")),
            signature: None,
            node_type: "mcp_server".to_string(),
        });
        b.edges.push(ExtractedEdge {
            source: config_id.clone(),
            target: server_id.clone(),
            relation: "contains".to_string(),
            confidence: "EXTRACTED".to_string(),
            confidence_score: Some(1.0),
            source_file: b.file_path.clone(),
            source_line: Some(1),
        });

        // Launch command (safe to store — it's how the server starts).
        if let Some(cmd) = cfg.get("command").and_then(|c| c.as_str()) {
            let cmd_id = format!("mcp_cmd_{}::{}", normalize_id(&stem), normalize_id(cmd));
            b.nodes.push(ExtractedNode {
                id: cmd_id.clone(),
                label: cmd.to_string(),
                source_file: b.file_path.clone(),
                source_line: Some(1),
                docstring: None,
                signature: None,
                node_type: "mcp_command".to_string(),
            });
            b.edges.push(ExtractedEdge {
                source: server_id.clone(),
                target: cmd_id.clone(),
                relation: "references".to_string(),
                confidence: "EXTRACTED".to_string(),
                confidence_score: Some(1.0),
                source_file: b.file_path.clone(),
                source_line: Some(1),
            });
        }

        // Package identity from args (npm scope/name or python module).
        let args: Vec<String> = cfg
            .get("args")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        let mut seen_pkg: Option<String> = None;
        for arg in &args {
            if let Some((pid, pname)) = package_re(arg) {
                if seen_pkg.as_deref() != Some(pname.as_str()) {
                    seen_pkg = Some(pname.clone());
                    b.nodes.push(ExtractedNode {
                        id: pid.clone(),
                        label: pname,
                        source_file: b.file_path.clone(),
                        source_line: Some(1),
                        docstring: None,
                        signature: None,
                        node_type: "mcp_package".to_string(),
                    });
                    b.edges.push(ExtractedEdge {
                        source: server_id.clone(),
                        target: pid,
                        relation: "references".to_string(),
                        confidence: "EXTRACTED".to_string(),
                        confidence_score: Some(1.0),
                        source_file: b.file_path.clone(),
                        source_line: Some(1),
                    });
                }
            }
        }

        // Env var NAMES only — never values.
        if let Some(env) = cfg.get("env").and_then(|e| e.as_object()) {
            for key in env.keys() {
                let env_id = format!("env_{}", normalize_id(key));
                if !b.nodes.iter().any(|n| n.id == env_id) {
                    b.nodes.push(ExtractedNode {
                        id: env_id.clone(),
                        label: key.clone(),
                        source_file: b.file_path.clone(),
                        source_line: Some(1),
                        docstring: None,
                        signature: None,
                        node_type: "env_var".to_string(),
                    });
                }
                b.edges.push(ExtractedEdge {
                    source: server_id.clone(),
                    target: env_id,
                    relation: "requires_env".to_string(),
                    confidence: "EXTRACTED".to_string(),
                    confidence_score: Some(1.0),
                    source_file: b.file_path.clone(),
                    source_line: Some(1),
                });
            }
        }
    }
}

fn ingest_go_mod(text: &str, b: &mut ManifestBuilder) {
    let mut module_name = None;
    let mut in_block = false;
    let mut requires: Vec<String> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("module ") {
            module_name = Some(rest.trim().to_string());
        }
        if line.starts_with("require (") {
            in_block = true;
            continue;
        }
        if in_block && line == ")" {
            in_block = false;
            continue;
        }
        let entry = if in_block {
            Some(line)
        } else {
            line.strip_prefix("require ").map(|r| r.trim())
        };
        if let Some(entry) = entry {
            let mut parts = entry.split_whitespace();
            if let Some(dep) = parts.next() {
                let indirect = parts
                    .last()
                    .map(|l| l.contains("indirect"))
                    .unwrap_or(false);
                if !indirect {
                    requires.push(dep.to_string());
                }
            }
        }
    }

    let self_id = b.add_package(
        module_name.as_deref().unwrap_or("unknown-go-module"),
        Some("this module (go.mod)"),
    );
    for dep in requires {
        b.add_dependency(&self_id, &dep);
    }
}

fn ingest_package_json(text: &str, b: &mut ManifestBuilder) {
    let Ok(doc) = text.parse::<serde_json::Value>() else {
        return;
    };
    let name = doc
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown-npm-package");
    let self_id = b.add_package(name, Some("this package (package.json)"));
    for section in ["dependencies", "devDependencies"] {
        if let Some(deps) = doc.get(section).and_then(|v| v.as_object()) {
            for dep_name in deps.keys() {
                b.add_dependency(&self_id, dep_name);
            }
        }
    }
}

/// The parts of a `pom.xml` the graph keeps: the project's own artifact and
/// the artifacts its dependencies name.
struct Pom {
    artifact: String,
    dependencies: Vec<String>,
}

/// A text run that is the direct content of an element the project owns.
#[derive(Clone, Copy)]
enum PomText {
    /// `<groupId>`/`<artifactId>`/`<version>` of the project itself.
    Project(&'static str),
    /// The same under `<parent>`: inherited coordinates, not its identity.
    Parent(&'static str),
    /// One entry under `<properties>`, keyed by the element's own name.
    Property,
    /// The `<artifactId>` of one of the project's `<dependency>` elements.
    DependencyArtifact,
}

/// Parse `pom.xml` as XML rather than by pattern, so namespaces (default or
/// prefixed, in either quote style) and element scope fall out of the
/// structure:
///
/// - Only the root element's own namespace is Maven's. A tag in a foreign
///   namespace is ignored however it is spelled, and so is an unprefixed tag
///   under a foreign default namespace.
/// - `<parent>` supplies coordinates a project inherits, never its own name.
/// - Only the project's own `<dependencies>` (and those of its profiles) are
///   dependencies: `<dependencyManagement>` pins versions, a plugin's
///   dependencies belong to the build tool, and an `<exclusion>` names what
///   is left out.
/// - `${...}` placeholders resolve from `<properties>` and then the
///   project's coordinates; an unknown placeholder is kept as written.
///
/// Malformed input keeps whatever parsed up to the error.
fn parse_pom(text: &str) -> Pom {
    let mut pom = Pom {
        artifact: String::new(),
        dependencies: Vec::new(),
    };
    let mut parent_artifact = String::new();
    let mut properties: Vec<(String, String)> = Vec::new();
    let mut coordinates: Vec<(String, String)> = Vec::new();

    let mut reader = quick_xml::Reader::from_str(text);
    reader.config_mut().trim_text(true);
    // Open elements, innermost last. An element outside the root's
    // namespace is `None`, so nothing below it can pass for Maven's.
    let mut path: Vec<Option<String>> = Vec::new();
    // Namespace bindings in scope: (prefix, URI), an empty prefix being the
    // default namespace.
    let mut bindings: Vec<(String, String)> = Vec::new();
    // How many bindings each open element declared, popped when it closes.
    let mut declared: Vec<usize> = Vec::new();
    // `Err` until the root element has been seen.
    let mut root_namespace: Result<Option<String>, ()> = Err(());
    // The element the next text run belongs to, and — for a property — the
    // element's own name, which is the property's key.
    let mut capture: Option<PomText> = None;
    let mut property_name: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(event)) => {
                // Declarations on an element apply to the element itself.
                let declared_here = bind_namespaces(&event, &mut bindings);
                let (prefix, local) = element_name(&event);
                let namespace = lookup_namespace(prefix.as_deref(), &bindings);
                if root_namespace.is_err() {
                    root_namespace = Ok(namespace.clone());
                }
                let ours = matches!(&root_namespace, Ok(root) if *root == namespace);
                // The open elements are exactly `ancestors`, which makes the
                // current element a direct child of that path.
                let within = |ancestors: &[&str]| {
                    path.len() == ancestors.len()
                        && path
                            .iter()
                            .zip(ancestors)
                            .all(|(open, name)| open.as_deref() == Some(*name))
                };
                capture = None;
                if ours {
                    let coordinate = match local.as_str() {
                        "groupId" => Some("groupId"),
                        "artifactId" => Some("artifactId"),
                        "version" => Some("version"),
                        _ => None,
                    };
                    if within(&["project"]) {
                        capture = coordinate.map(PomText::Project);
                    } else if within(&["project", "parent"]) {
                        capture = coordinate.map(PomText::Parent);
                    } else if within(&["project", "properties"]) {
                        capture = Some(PomText::Property);
                        property_name = Some(local.clone());
                    } else if local == "artifactId"
                        && (within(&["project", "dependencies", "dependency"])
                            || within(&[
                                "project",
                                "profiles",
                                "profile",
                                "dependencies",
                                "dependency",
                            ]))
                    {
                        capture = Some(PomText::DependencyArtifact);
                    }
                }
                path.push(ours.then(|| local.clone()));
                declared.push(declared_here);
            }
            Ok(quick_xml::events::Event::Empty(event)) => {
                let declared_here = bind_namespaces(&event, &mut bindings);
                bindings.truncate(bindings.len().saturating_sub(declared_here));
            }
            Ok(quick_xml::events::Event::End(_)) => {
                capture = None;
                if path.pop().is_some() {
                    let count = declared.pop().unwrap_or(0);
                    bindings.truncate(bindings.len().saturating_sub(count));
                }
            }
            Ok(quick_xml::events::Event::Text(t)) => {
                let Some(kind) = capture.take() else {
                    continue;
                };
                let Ok(decoded) = t.decode() else {
                    continue;
                };
                let Ok(value) = quick_xml::escape::unescape(&decoded) else {
                    continue;
                };
                let value = value.trim().to_string();
                if value.is_empty() {
                    continue;
                }
                match kind {
                    PomText::Project(field) => {
                        if field == "artifactId" {
                            pom.artifact = value.clone();
                        }
                        coordinates.push((format!("project.{field}"), value));
                    }
                    PomText::Parent(field) => {
                        if field == "artifactId" {
                            parent_artifact = value.clone();
                        }
                        coordinates.push((format!("project.parent.{field}"), value));
                    }
                    PomText::Property => {
                        if let Some(name) = property_name.take() {
                            properties.push((name, value));
                        }
                    }
                    PomText::DependencyArtifact => pom.dependencies.push(value),
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }

    let placeholder = |value: &str| resolve_pom_placeholder(value, &properties, &coordinates);
    pom.artifact = placeholder(&pom.artifact);
    // A project without its own `<artifactId>` is known by what it inherits.
    if pom.artifact.is_empty() {
        pom.artifact = placeholder(&parent_artifact);
    }
    pom.dependencies
        .iter_mut()
        .for_each(|dependency| *dependency = placeholder(dependency));
    pom.dependencies.retain(|dependency| !dependency.is_empty());
    pom
}

/// Record the namespace declarations on an element and return how many were
/// added to `bindings`.
fn bind_namespaces(
    event: &quick_xml::events::BytesStart<'_>,
    bindings: &mut Vec<(String, String)>,
) -> usize {
    let mut declared = 0;
    for attr in event.attributes().flatten() {
        let is_default = attr.key.prefix().is_none() && attr.key.local_name().as_ref() == b"xmlns";
        let is_prefixed = attr
            .key
            .prefix()
            .is_some_and(|prefix| prefix.as_ref() == b"xmlns");
        if !(is_default || is_prefixed) {
            continue;
        }
        let Ok(uri) = attr.normalized_value(quick_xml::XmlVersion::Explicit1_0) else {
            continue;
        };
        let prefix = if is_default {
            String::new()
        } else {
            String::from_utf8_lossy(attr.key.local_name().as_ref()).into_owned()
        };
        bindings.push((prefix, uri.into_owned()));
        declared += 1;
    }
    declared
}

/// The prefix and local name of an element's name.
fn element_name(event: &quick_xml::events::BytesStart<'_>) -> (Option<String>, String) {
    let name = event.name();
    (
        name.prefix()
            .map(|prefix| String::from_utf8_lossy(prefix.as_ref()).into_owned()),
        String::from_utf8_lossy(name.local_name().as_ref()).into_owned(),
    )
}

/// The URI a prefix is bound to; no prefix takes the default namespace in
/// scope. `None` means no namespace at all — an unbound prefix is malformed
/// and lands here too, which only makes its element foreign.
fn lookup_namespace(prefix: Option<&str>, bindings: &[(String, String)]) -> Option<String> {
    bindings
        .iter()
        .rev()
        .find(|(bound, _)| bound.as_str() == prefix.unwrap_or(""))
        .map(|(_, uri)| uri.clone())
}

/// Replace each `${name}` with the property of that name, from
/// `<properties>` first and then the project's coordinates. One pass, no
/// recursion: a placeholder is either known or kept as written.
fn resolve_pom_placeholder(
    value: &str,
    properties: &[(String, String)],
    coordinates: &[(String, String)],
) -> String {
    if !value.contains("${") {
        return value.to_string();
    }
    let mut resolved = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        resolved.push_str(&rest[..start]);
        let Some(length) = rest[start..].find('}').map(|end| end + 1) else {
            break;
        };
        let name = &rest[start + 2..start + length - 1];
        let replacement = properties
            .iter()
            .chain(coordinates)
            .find(|(known, _)| known == name)
            .map(|(_, value)| value.as_str())
            .unwrap_or(&rest[start..start + length]);
        resolved.push_str(replacement);
        rest = &rest[start + length..];
    }
    resolved.push_str(rest);
    resolved
}

fn ingest_pom(text: &str, b: &mut ManifestBuilder) {
    let pom = parse_pom(text);
    let self_name = if pom.artifact.is_empty() {
        "unknown-maven-artifact"
    } else {
        pom.artifact.as_str()
    };
    let self_id = b.add_package(self_name, Some("this artifact (pom.xml)"));
    for dependency in &pom.dependencies {
        b.add_dependency(&self_id, dependency);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pyproject_produces_package_and_deps() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("pyproject.toml");
        std::fs::write(
            &p,
            "[project]\nname = \"my-app\"\ndependencies = [\"httpx>=0.27\", \"rich\"]\n",
        )
        .unwrap();
        let ext = extract_manifest(&p, dir.path());
        assert_eq!(ext.language, "Manifest");
        assert!(ext.nodes.iter().any(|n| n.id == "pkg_my_app"));
        assert!(ext.nodes.iter().any(|n| n.id == "pkg_httpx"));
        assert!(ext.nodes.iter().any(|n| n.id == "pkg_rich"));
        assert!(ext.edges.iter().any(|e| e.relation == "depends_on"
            && e.source == "pkg_my_app"
            && e.target == "pkg_httpx"));
    }

    #[test]
    fn package_json_deps_and_self() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("package.json");
        std::fs::write(
            &p,
            r#"{"name": "@scope/cli", "dependencies": {"commander": "^12.0.0"}}"#,
        )
        .unwrap();
        let ext = extract_manifest(&p, dir.path());
        assert!(ext.nodes.iter().any(|n| n.id == "pkg_scope_cli"));
        assert!(ext.nodes.iter().any(|n| n.id == "pkg_commander"));
        assert_eq!(
            ext.edges
                .iter()
                .filter(|e| e.relation == "depends_on")
                .count(),
            1
        );
    }

    #[test]
    fn go_mod_skips_indirect() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("go.mod");
        std::fs::write(
            &p,
            "module example.com/app\n\ngo 1.22\n\nrequire (\n\tgithub.com/foo/bar v1.0.0\n\tgithub.com/indirect/dep v0.1.0 // indirect\n)\n",
        )
        .unwrap();
        let ext = extract_manifest(&p, dir.path());
        assert!(ext.nodes.iter().any(|n| n.id == "pkg_example_com_app"));
        assert!(ext.nodes.iter().any(|n| n.id == "pkg_github_com_foo_bar"));
        assert!(!ext.nodes.iter().any(|n| n.id.contains("indirect")));
    }

    #[test]
    fn cargo_workspace_members_read_names() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("Cargo.toml");
        std::fs::write(
            &ws,
            "[workspace]\nmembers = [\"crates/core\", \"crates/cli\"]\n",
        )
        .unwrap();
        let core = dir.path().join("crates/core/Cargo.toml");
        std::fs::create_dir_all(core.parent().unwrap()).unwrap();
        std::fs::write(
            &core,
            "[package]\nname = \"astria-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        // cli member has no Cargo.toml — falls back to dir-derived name
        std::fs::create_dir_all(dir.path().join("crates/cli")).unwrap();

        let ext = extract_manifest(&ws, dir.path());
        assert!(ext.nodes.iter().any(|n| n.id == "pkg_astria_core"));
        assert!(ext.nodes.iter().any(|n| n.id == "pkg_crates_cli"));
        assert!(ext
            .edges
            .iter()
            .any(|e| e.relation == "contains" && e.target == "pkg_astria_core"));
    }

    #[test]
    fn pom_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("pom.xml");
        std::fs::write(
            &p,
            "<project><artifactId>my-app</artifactId><dependencies><dependency><groupId>org.x</groupId><artifactId>junit</artifactId></dependency></dependencies></project>",
        )
        .unwrap();
        let ext = extract_manifest(&p, dir.path());
        assert!(ext.nodes.iter().any(|n| n.id == "pkg_my_app"));
        assert!(ext.nodes.iter().any(|n| n.id == "pkg_junit"));
    }

    #[test]
    fn pom_namespaces_and_scopes_are_honored() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("pom.xml");
        std::fs::write(
            &p,
            r#"<m:project xmlns:m="http://maven.apache.org/POM/4.0.0" xmlns:x="http://foreign.example/">
  <m:parent><m:groupId>org.parent</m:groupId><m:artifactId>parent-app</m:artifactId></m:parent>
  <m:artifactId>my-app</m:artifactId>
  <m:dependencyManagement><m:dependencies><m:dependency>
    <m:artifactId>managed-lib</m:artifactId>
  </m:dependency></m:dependencies></m:dependencyManagement>
  <m:build><m:plugins><m:plugin><m:artifactId>plugin-a</m:artifactId>
    <m:dependencies><m:dependency><m:artifactId>plugin-dep</m:artifactId></m:dependency></m:dependencies>
  </m:plugin></m:plugins></m:build>
  <m:dependencies>
    <m:dependency><m:groupId>org.x</m:groupId><m:artifactId>junit</m:artifactId>
      <m:exclusions><m:exclusion><m:artifactId>excluded-lib</m:artifactId></m:exclusion></m:exclusions>
    </m:dependency>
    <x:dependency><x:artifactId>foreign-lib</x:artifactId></x:dependency>
  </m:dependencies>
  <m:profiles><m:profile><m:dependencies><m:dependency>
    <m:artifactId>profile-lib</m:artifactId>
  </m:dependency></m:dependencies></m:profile></m:profiles>
</m:project>"#,
        )
        .unwrap();
        let ext = extract_manifest(&p, dir.path());
        let is_package = |name: &str| ext.nodes.iter().any(|n| n.id == pkg_id(name));
        let is_dependency = |name: &str| {
            ext.edges
                .iter()
                .any(|e| e.relation == "depends_on" && e.target == pkg_id(name))
        };
        // The project's own identity, not its parent's.
        assert!(is_package("my-app"), "nodes: {:?}", ext.nodes);
        assert!(!is_package("parent-app"), "nodes: {:?}", ext.nodes);
        for dep in ["junit", "profile-lib"] {
            assert!(
                is_dependency(dep),
                "missing dependency {dep}: {:?}",
                ext.edges
            );
        }
        // Managed versions, build plugins, exclusions and foreign-namespace
        // tags are not dependencies of the project, however they are spelled.
        for not_dep in [
            "managed-lib",
            "plugin-a",
            "plugin-dep",
            "excluded-lib",
            "foreign-lib",
        ] {
            assert!(
                !is_dependency(not_dep),
                "{not_dep} is not a project dependency: {:?}",
                ext.edges
            );
        }
    }

    #[test]
    fn pom_properties_resolve_placeholders() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("pom.xml");
        // Single-quoted and spaced `xmlns`, and a comment that spells a tag.
        std::fs::write(
            &p,
            r#"<project xmlns = 'http://maven.apache.org/POM/4.0.0'>
  <!-- <artifactId>ghost</artifactId> -->
  <artifactId>my-app</artifactId>
  <properties><dep.name>commons-lang3</dep.name></properties>
  <dependencies><dependency><artifactId>${dep.name}</artifactId></dependency></dependencies>
</project>"#,
        )
        .unwrap();
        let ext = extract_manifest(&p, dir.path());
        assert!(
            ext.nodes.iter().any(|n| n.id == pkg_id("my-app")),
            "nodes: {:?}",
            ext.nodes
        );
        assert!(
            !ext.nodes.iter().any(|n| n.id == pkg_id("ghost")),
            "nodes: {:?}",
            ext.nodes
        );
        assert!(
            ext.edges
                .iter()
                .any(|e| e.relation == "depends_on" && e.target == pkg_id("commons-lang3")),
            "edges: {:?}",
            ext.edges
        );
    }

    #[test]
    fn pom_without_namespace_ignores_prefixed_lookalikes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("pom.xml");
        std::fs::write(
            &p,
            r#"<project>
  <artifactId>plain-app</artifactId>
  <dependencies>
    <dependency><artifactId>real-dep</artifactId></dependency>
    <ext:dependency xmlns:ext="http://other.example/"><ext:artifactId>other-dep</ext:artifactId></ext:dependency>
  </dependencies>
</project>"#,
        )
        .unwrap();
        let ext = extract_manifest(&p, dir.path());
        assert!(
            ext.nodes.iter().any(|n| n.id == pkg_id("plain-app")),
            "nodes: {:?}",
            ext.nodes
        );
        assert!(ext
            .edges
            .iter()
            .any(|e| e.relation == "depends_on" && e.target == pkg_id("real-dep")));
        assert!(
            !ext.edges
                .iter()
                .any(|e| e.relation == "depends_on" && e.target == pkg_id("other-dep")),
            "edges: {:?}",
            ext.edges
        );
    }

    #[test]
    fn manifest_detection_by_filename() {
        assert!(is_manifest(Path::new("any/dir/pyproject.toml")));
        assert!(is_manifest(Path::new("Cargo.TOML")));
        assert!(is_manifest(Path::new("go.mod")));
        assert!(is_manifest(Path::new(".mcp.json")));
        assert!(is_manifest(Path::new("mcp_servers.json")));
        assert!(!is_manifest(Path::new("main.rs")));
        assert!(!is_manifest(Path::new("README.md")));
    }

    #[test]
    fn mcp_config_ingests_servers_env_names_not_values() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(".mcp.json");
        std::fs::write(
            &p,
            r#"{"mcpServers":{"fs":{"command":"npx","args":["-y","@modelcontextprotocol/server-fs"],"env":{"FS_TOKEN":"super-secret-value","HOME":"/x"}}}}"#,
        )
        .unwrap();
        let ext = extract_manifest(&p, dir.path());
        assert!(ext.nodes.iter().any(|n| n.node_type == "mcp_config"));
        assert!(ext.nodes.iter().any(|n| n.label == "mcp:fs"));
        assert!(ext.nodes.iter().any(|n| n.label == "npx"));
        assert!(ext
            .nodes
            .iter()
            .any(|n| n.node_type == "mcp_package" && n.label.contains("server-fs")));
        // Env var NAMES become nodes; the secret VALUE must never appear.
        let env_names: Vec<&str> = ext
            .nodes
            .iter()
            .filter(|n| n.node_type == "env_var")
            .map(|n| n.label.as_str())
            .collect();
        assert_eq!(env_names, vec!["FS_TOKEN", "HOME"]);
        let leak_check = ext
            .nodes
            .iter()
            .map(|n| n.label.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .to_string();
        assert!(!leak_check.contains("super-secret-value"));
        assert!(ext.edges.iter().any(|e| e.relation == "requires_env"));
    }

    #[test]
    fn cargo_path_deps_link_member_crates() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("Cargo.toml");
        std::fs::write(
            &ws,
            "[workspace]\nmembers = [\"crates/core\", \"crates/cli\"]\n",
        )
        .unwrap();
        let core = dir.path().join("crates/core/Cargo.toml");
        std::fs::create_dir_all(core.parent().unwrap()).unwrap();
        std::fs::write(
            &core,
            "[package]\nname = \"astria-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let cli = dir.path().join("crates/cli/Cargo.toml");
        std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
        std::fs::write(
            &cli,
            "[package]\nname = \"astria-cli\"\n\n[dependencies]\nastria-core = { path = \"../core\" }\n",
        )
        .unwrap();

        let ext = extract_manifest(&cli, dir.path());
        assert!(
            ext.edges.iter().any(|e| e.relation == "crate_depends_on"
                && e.source == "pkg_astria_cli"
                && e.target == "pkg_astria_core"),
            "edges: {:?}",
            ext.edges
        );
    }

    #[test]
    fn cargo_workspace_true_inherits_path_dep() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("Cargo.toml");
        std::fs::write(
            &ws,
            "[workspace]\nmembers = [\"crates/core\", \"crates/cli\"]\n\n[workspace.dependencies]\nastria-core = { path = \"crates/core\" }\n",
        )
        .unwrap();
        let core = dir.path().join("crates/core/Cargo.toml");
        std::fs::create_dir_all(core.parent().unwrap()).unwrap();
        std::fs::write(
            &core,
            "[package]\nname = \"astria-core\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let cli = dir.path().join("crates/cli/Cargo.toml");
        std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
        std::fs::write(
            &cli,
            "[package]\nname = \"astria-cli\"\n\n[dependencies]\nastria-core = { workspace = true }\n",
        )
        .unwrap();

        let ext = extract_manifest(&cli, dir.path());
        assert!(ext
            .edges
            .iter()
            .any(|e| e.relation == "crate_depends_on" && e.target == "pkg_astria_core"));
    }

    #[test]
    fn manifest_paths_cannot_leave_the_scanned_tree() {
        // `members` and `path` dependencies are read from the manifest, not
        // discovered by the scanner: a value that climbs out of the scanned
        // tree names a file astria has no business opening.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ws");
        std::fs::create_dir_all(root.join("crates/cli")).unwrap();
        std::fs::create_dir_all(root.join("crates/core")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/core\", \"../outside\"]\n",
        )
        .unwrap();
        std::fs::write(
            root.join("crates/core/Cargo.toml"),
            "[package]\nname = \"in-root\"\n",
        )
        .unwrap();
        let outside = dir.path().join("outside/Cargo.toml");
        std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
        std::fs::write(&outside, "[package]\nname = \"sibling-crate\"\n").unwrap();
        std::fs::write(
            root.join("crates/cli/Cargo.toml"),
            "[package]\nname = \"cli\"\n\n[dependencies]\nsibling = { path = \"../../../outside\" }\n",
        )
        .unwrap();

        let workspace = extract_manifest(&root.join("Cargo.toml"), &root);
        assert!(
            !workspace.nodes.iter().any(|n| n.id == "pkg_sibling_crate"),
            "a member outside the scanned tree was read: {:?}",
            workspace.nodes
        );
        let cli = extract_manifest(&root.join("crates/cli/Cargo.toml"), &root);
        assert!(
            !cli.edges
                .iter()
                .any(|e| e.relation == "crate_depends_on" && e.target == "pkg_sibling_crate"),
            "a path dependency outside the scanned tree was read: {:?}",
            cli.edges
        );
    }

    #[test]
    fn absolute_manifest_paths_keep_the_directory_derived_name() {
        // Nothing here exists and nothing is looked up: an absolute or rooted
        // spelling is refused before the operating system sees it, and the
        // member still gets its node, named after the directory.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ws");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"//host/share/crate\", \"/absolute/crate\", \"C:\\\\elsewhere\\\\crate\"]\n",
        )
        .unwrap();

        let ext = extract_manifest(&root.join("Cargo.toml"), &root);
        let members = ext
            .nodes
            .iter()
            .filter(|n| n.docstring.as_deref() == Some("workspace member"))
            .count();
        assert_eq!(members, 3, "nodes: {:?}", ext.nodes);
    }
}
