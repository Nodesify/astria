use super::snapshot::{is_code, Sources};
use super::{ChangedDeclaration, Consumer, EvidenceStep};
use astria_extract::{ExtractedNode, Extraction};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::Path;

pub(super) struct Graph {
    pub nodes: HashMap<String, ExtractedNode>,
    ranges: HashMap<String, (u32, u32)>,
    by_file: BTreeMap<String, Vec<String>>,
    incoming: HashMap<String, Vec<astria_extract::ExtractedEdge>>,
    members: HashMap<String, Vec<astria_extract::ExtractedEdge>>,
    owners: Vec<OwnerRule>,
    pub issues: Vec<String>,
    pub files_indexed: usize,
    pub identity: String,
    pub unresolved_edges: usize,
}

struct OwnerRule {
    matcher: ignore::gitignore::Gitignore,
    names: Vec<String>,
    source: String,
}

fn owners(files: &BTreeMap<String, Vec<u8>>) -> Vec<OwnerRule> {
    let Some((path, bytes)) = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"]
        .into_iter()
        .find_map(|path| files.get(path).map(|bytes| (path, bytes)))
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(bytes)
        .lines()
        .enumerate()
        .filter_map(|(line, text)| {
            let text = text.trim();
            if text.is_empty() || text.starts_with('#') {
                return None;
            }
            let mut fields = text.split_whitespace();
            let pattern = fields.next()?;
            // CODEOWNERS does not support negation, character ranges or escaped spaces.
            if pattern.starts_with('!') || pattern.contains('[') || pattern.contains('\\') {
                return None;
            }
            let names = fields
                .take_while(|v| !v.starts_with('#'))
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let mut builder = ignore::gitignore::GitignoreBuilder::new("");
            builder.add_line(None, pattern).ok()?;
            Some(OwnerRule {
                matcher: builder.build().ok()?,
                names,
                source: format!("{path}:{}", line + 1),
            })
        })
        .collect()
}

pub(super) fn build(sources: Sources) -> Graph {
    let mut graph = Graph {
        nodes: HashMap::new(),
        ranges: HashMap::new(),
        by_file: BTreeMap::new(),
        incoming: HashMap::new(),
        members: HashMap::new(),
        owners: owners(&sources.files),
        issues: sources.issues,
        files_indexed: 0,
        identity: sources.identity,
        unresolved_edges: 0,
    };
    let mut extractions = Vec::<Extraction>::new();
    // Use the ignore policy at each revision, not the caller's current policy.
    let mut ignore_builder = ignore::gitignore::GitignoreBuilder::new("");
    if let Some(bytes) = sources.files.get(".astriaignore") {
        for line in String::from_utf8_lossy(bytes).lines() {
            if let Err(e) = ignore_builder.add_line(None, line) {
                graph.issues.push(format!(".astriaignore: {e}"));
            }
        }
    }
    let ignores = ignore_builder.build();
    if let Err(e) = &ignores {
        graph.issues.push(format!(".astriaignore: {e}"));
    }
    for (path, bytes) in sources.files {
        if !is_code(&path) {
            continue;
        }
        if ignores
            .as_ref()
            .is_ok_and(|m| m.matched_path_or_any_parents(&path, false).is_ignore())
        {
            // Exclusion is explicit coverage information: it can hide consumers,
            // so an incomplete report must not become a successful zero-risk gate.
            graph
                .issues
                .push(format!("{path}: excluded by revision .astriaignore"));
            continue;
        }
        let file = Path::new(&path);
        let ext = file
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let Some(cfg) = astria_extract::langs::get_language_for_extension(&ext) else {
            continue;
        };
        if matches!(
            ext.as_str(),
            "vue" | "svelte" | "astro" | "vb" | "pas" | "dpr" | "dpk" | "inc"
        ) || !cfg.compiled_in
        {
            graph.issues.push(format!(
                "{path}: {} has no byte-based structural review parser",
                cfg.name
            ));
            continue;
        }
        let spans = match astria_extract::declaration_spans(cfg, &bytes) {
            Ok(spans) => spans,
            Err(e) => {
                graph.issues.push(format!("{path}: {e}"));
                continue;
            }
        };
        match astria_extract::extract_source(file, cfg, file, &bytes) {
            Ok(extraction) => {
                let line_count = bytes.iter().filter(|b| **b == b'\n').count() as u32 + 1;
                for node in &extraction.nodes {
                    if node.node_type == "file" {
                        graph.ranges.insert(node.id.clone(), (1, line_count));
                    } else if matches!(
                        node.node_type.as_str(),
                        "function" | "class" | "test" | "constant"
                    ) {
                        if let Some(line) = node.source_line {
                            let candidates: Vec<_> =
                                spans.iter().filter(|(start, _)| *start == line).collect();
                            if candidates.len() == 1 {
                                graph.ranges.insert(node.id.clone(), *candidates[0]);
                            } else {
                                // Multiple declarations on one physical line
                                // need byte-range identity. Never assign a class
                                // the shorter range of its same-line method.
                                graph.issues.push(format!(
                                    "{path}:{line}: declaration span unavailable or ambiguous for {}",
                                    node.label
                                ));
                            }
                        }
                    }
                }
                graph.files_indexed += 1;
                extractions.push(extraction);
            }
            Err(e) => graph.issues.push(format!("{path}: {e}")),
        }
    }
    astria_extract::resolve_cross_file_references(&mut extractions);
    for extraction in extractions {
        let file = extraction.file_path.to_string_lossy().replace('\\', "/");
        for node in extraction.nodes {
            graph
                .by_file
                .entry(file.clone())
                .or_default()
                .push(node.id.clone());
            graph.nodes.insert(node.id.clone(), node);
        }
        for edge in extraction.edges {
            if matches!(edge.relation.as_str(), "contains" | "method") {
                graph
                    .members
                    .entry(edge.source.clone())
                    .or_default()
                    .push(edge);
                continue;
            }
            if matches!(
                edge.relation.as_str(),
                "calls"
                    | "references"
                    | "imports"
                    | "imports_from"
                    | "inherits"
                    | "uses"
                    | "depends_on"
                    | "requires"
            ) {
                graph
                    .incoming
                    .entry(edge.target.clone())
                    .or_default()
                    .push(edge);
            }
        }
    }
    graph.unresolved_edges = graph
        .incoming
        .iter()
        .filter(|(target, _)| !graph.nodes.contains_key(*target))
        .map(|(_, edges)| edges.len())
        .sum();
    graph
}

impl Graph {
    pub fn select(&self, path: &str, hunks: &[(u32, u32)], whole_file: bool) -> BTreeSet<String> {
        let ids = self.by_file.get(path).map(Vec::as_slice).unwrap_or(&[]);
        let mut selected = BTreeSet::new();
        if whole_file {
            selected.extend(
                ids.iter()
                    .filter(|id| self.ranges.contains_key(*id))
                    .cloned(),
            );
            return selected;
        }
        for &(first, last) in hunks {
            // Each changed line belongs to its innermost declaration. A nested
            // method edit must not seed every sibling through its containing class.
            for line in first..=last {
                let candidates = ids
                    .iter()
                    .filter_map(|id| {
                        self.ranges
                            .get(id)
                            .filter(|(a, b)| *a <= line && line <= *b)
                            .map(|(a, b)| (id, b - a))
                    })
                    .collect::<Vec<_>>();
                if let Some(minimum) = candidates.iter().map(|(_, width)| width).min() {
                    selected.extend(
                        candidates
                            .iter()
                            .filter(|(_, width)| width == minimum)
                            .map(|(id, _)| (*id).clone()),
                    );
                }
            }
        }
        selected
    }

    fn ownership(&self, file: &str) -> (Vec<String>, Option<String>) {
        self.owners
            .iter()
            .rev()
            .find(|rule| {
                rule.matcher
                    .matched_path_or_any_parents(file, false)
                    .is_ignore()
            })
            .map(|rule| (rule.names.clone(), Some(rule.source.clone())))
            .unwrap_or_default()
    }

    pub fn declaration(&self, id: &str, snapshot: &str, change: &str) -> ChangedDeclaration {
        let node = &self.nodes[id];
        let file = node.source_file.to_string_lossy().replace('\\', "/");
        let (owners, owner_source) = self.ownership(&file);
        let (line, end_line) = self.ranges[id];
        ChangedDeclaration {
            id: id.into(),
            label: node.label.clone(),
            file,
            line,
            end_line,
            kind: node.node_type.clone(),
            snapshot: snapshot.into(),
            change: change.into(),
            owners,
            owner_source,
        }
    }

    pub fn consumers(
        &self,
        seeds: &BTreeSet<String>,
        snapshot: &str,
        after: &Graph,
    ) -> Vec<Consumer> {
        let mut results = BTreeMap::<String, Consumer>::new();
        // Traverse each seed independently so every review item retains the
        // actual changed declaration and a complete supporting chain.
        for seed in seeds {
            let mut visited = HashMap::from([(seed.clone(), (0u32, 0u8))]);
            let mut queue = VecDeque::from([(
                seed.clone(),
                Vec::<EvidenceStep>::new(),
                "EXTRACTED".to_owned(),
                0u32,
            )]);
            // A changed type signature can affect callers of its members. This
            // expansion applies only to a selected type, never to a method-only
            // edit or an unrelated declaration sharing the same source file.
            if self.nodes[seed].node_type == "class" {
                for member in self.members.get(seed).into_iter().flatten() {
                    if !self.nodes.contains_key(&member.target) {
                        continue;
                    }
                    visited.insert(member.target.clone(), (0, 0));
                    queue.push_back((
                        member.target.clone(),
                        vec![EvidenceStep {
                            from: member.source.clone(),
                            to: member.target.clone(),
                            relation: member.relation.clone(),
                            evidence: member.confidence.clone(),
                            file: member.source_file.to_string_lossy().replace('\\', "/"),
                            line: member.source_line,
                        }],
                        member.confidence.clone(),
                        0,
                    ));
                }
            }
            while let Some((current, path, evidence, depth)) = queue.pop_front() {
                for edge in self.incoming.get(&current).into_iter().flatten() {
                    let Some(node) = self.nodes.get(&edge.source) else {
                        continue;
                    };
                    let tier = weaker(&evidence, &edge.confidence).to_owned();
                    let priority = (depth + 1, evidence_weight(&tier));
                    if visited
                        .get(&edge.source)
                        .is_some_and(|best| *best <= priority)
                    {
                        continue;
                    }
                    visited.insert(edge.source.clone(), priority);
                    let mut chain = path.clone();
                    chain.push(EvidenceStep {
                        from: edge.source.clone(),
                        to: current.clone(),
                        relation: edge.relation.clone(),
                        evidence: edge.confidence.clone(),
                        file: edge.source_file.to_string_lossy().replace('\\', "/"),
                        line: edge.source_line,
                    });
                    let file = node.source_file.to_string_lossy().replace('\\', "/");
                    let (owners, owner_source) = self.ownership(&file);
                    let candidate = Consumer {
                        id: node.id.clone(),
                        label: node.label.clone(),
                        file: file.clone(),
                        line: node.source_line,
                        snapshot: snapshot.into(),
                        changed_id: seed.clone(),
                        depth: depth + 1,
                        evidence: tier.clone(),
                        still_present: after.nodes.contains_key(&node.id),
                        is_test: node.node_type == "test" || is_test_file(&file),
                        owners,
                        owner_source,
                        path: chain.clone(),
                    };
                    let key = format!("{}\0{}", seed, node.id);
                    results.insert(key, candidate);
                    queue.push_back((edge.source.clone(), chain, tier, depth + 1));
                }
            }
        }
        results.into_values().collect()
    }
}

fn weaker<'a>(left: &'a str, right: &'a str) -> &'a str {
    if evidence_weight(left) >= evidence_weight(right) {
        left
    } else {
        right
    }
}

fn evidence_weight(value: &str) -> u8 {
    match value {
        "DECLARED" | "EXTRACTED" => 0,
        "RESOLVED" => 1,
        _ => 2,
    }
}

fn is_test_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower
        .split('/')
        .any(|p| matches!(p, "test" | "tests" | "__tests__" | "spec" | "specs"))
        || lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.ends_with("_test.go")
        || Path::new(&lower)
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|p| p.starts_with("test_"))
}
