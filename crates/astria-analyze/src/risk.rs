//! Source-grounded change review. Both revisions are extracted in memory;
//! deleted APIs retain their old consumers without altering the checkout.
mod impact;
mod snapshot;

use astria_core::Result;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffScope {
    WorkingTree,
    Staged,
    Range { base: String, head: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub status: String,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedDeclaration {
    pub id: String,
    pub label: String,
    pub file: String,
    pub line: u32,
    pub end_line: u32,
    pub kind: String,
    pub snapshot: String,
    pub change: String,
    pub owners: Vec<String>,
    pub owner_source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceStep {
    pub from: String,
    pub to: String,
    pub relation: String,
    pub evidence: String,
    pub file: String,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Consumer {
    pub id: String,
    pub label: String,
    pub file: String,
    pub line: Option<u32>,
    pub snapshot: String,
    pub changed_id: String,
    pub depth: u32,
    pub evidence: String,
    pub still_present: bool,
    pub is_test: bool,
    pub owners: Vec<String>,
    pub owner_source: Option<String>,
    pub path: Vec<EvidenceStep>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewReport {
    pub schema_version: u32,
    /// Missing coverage has no numeric score: unknown is never zero.
    pub score: Option<u32>,
    pub level: String,
    pub scope: String,
    pub base_commit: String,
    pub requested_base_commit: String,
    pub head_commit: Option<String>,
    pub before_identity: String,
    pub after_identity: String,
    pub changed_files: Vec<ChangedFile>,
    pub declarations: Vec<ChangedDeclaration>,
    pub consumers: Vec<Consumer>,
    pub impacted: usize,
    pub direct_consumers: usize,
    pub inferred_consumers: usize,
    pub test_consumers: usize,
    pub coverage_complete: bool,
    pub coverage_issues: Vec<String>,
    pub unresolved_before: usize,
    pub unresolved_after: usize,
    pub files_indexed_before: usize,
    pub files_indexed_after: usize,
    pub indexing_ms: u64,
}

pub fn review(root: &Path, scope: &DiffScope) -> Result<ReviewReport> {
    let started = Instant::now();
    let provided_root = root.canonicalize()?;
    let root_output = snapshot::git(&provided_root, &["rev-parse", "--show-toplevel"])?;
    let root_text = std::str::from_utf8(&root_output)
        .map_err(|_| astria_core::AstriaError::Graph("repository root is not UTF-8".into()))?;
    let root = Path::new(root_text.trim_end_matches(['\r', '\n'])).canonicalize()?;
    let head_now = snapshot::resolve_commit(&root, "HEAD")?;
    let (requested_base, base, head) = match scope {
        DiffScope::Range { base, head } => {
            let base = snapshot::resolve_commit(&root, base)?;
            let head = snapshot::resolve_commit(&root, head)?;
            let common = snapshot::merge_base(&root, &base, &head)?;
            (base, common, head)
        }
        _ => (head_now.clone(), head_now.clone(), head_now.clone()),
    };
    let changes = snapshot::changes(&root, scope, &base, &head)?;
    let before_sources = snapshot::load(&root, snapshot::Revision::Commit(&base))?;
    let after_revision = || match scope {
        DiffScope::Range { .. } => snapshot::Revision::Commit(&head),
        DiffScope::Staged => snapshot::Revision::Index,
        DiffScope::WorkingTree => snapshot::Revision::WorkingTree,
    };
    let after_sources = snapshot::load(&root, after_revision())?;
    let before = impact::build(before_sources);
    let after = impact::build(after_sources);
    let mut issues = Vec::new();
    issues.extend(before.issues.iter().map(|s| format!("before: {s}")));
    issues.extend(after.issues.iter().map(|s| format!("after: {s}")));
    let mut declarations = Vec::new();
    let mut old_seeds = BTreeSet::new();
    let mut new_seeds = BTreeSet::new();
    for change in &changes {
        let (old_hunks, new_hunks) = snapshot::hunks(&root, scope, &base, &head, change)?;
        let no_hunks = old_hunks.is_empty() && new_hunks.is_empty();
        let moved = change.old_path != change.new_path;
        for (graph, path, hunks, seeds, version, other) in [
            (
                &before,
                &change.old_path,
                &old_hunks,
                &mut old_seeds,
                "before",
                &after,
            ),
            (
                &after,
                &change.new_path,
                &new_hunks,
                &mut new_seeds,
                "after",
                &before,
            ),
        ] {
            let Some(path) = path else { continue };
            if !snapshot::is_code(path) {
                issues.push(format!(
                    "{version}: {path}: change has no structural dependency model"
                ));
                continue;
            }
            let selected = graph.select(path, hunks, moved || change.status == "untracked");
            if selected.is_empty()
                && change.old_path == change.new_path
                && no_hunks
                && change.status != "untracked"
            {
                // Includes mode-only and binary changes: not represented by line
                // hunks and therefore outside the declaration impact model.
                issues.push(format!(
                    "{version}: {path}: metadata/binary change has no declaration hunks"
                ));
            }
            if selected.is_empty() && (moved || !hunks.is_empty()) {
                issues.push(format!(
                    "{version}: {path}: changed source could not be mapped to declarations"
                ));
            }
            for id in selected {
                let action = if change.status.starts_with('R') {
                    "renamed"
                } else if other.nodes.contains_key(&id) {
                    "modified"
                } else if version == "before" {
                    "removed"
                } else {
                    "added"
                };
                declarations.push(graph.declaration(&id, version, action));
                seeds.insert(id);
            }
        }
    }
    let mut consumers = before.consumers(&old_seeds, "before", &after);
    consumers.extend(after.consumers(&new_seeds, "after", &after));
    consumers.sort_by(|a, b| {
        a.depth
            .cmp(&b.depth)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.snapshot.cmp(&b.snapshot))
            .then_with(|| a.changed_id.cmp(&b.changed_id))
    });
    declarations.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.snapshot.cmp(&b.snapshot))
    });
    // Detect concurrent source/index changes rather than certify a mixed snapshot.
    if !matches!(scope, DiffScope::Range { .. }) {
        let verified = snapshot::load(&root, after_revision())?;
        if verified.identity != after.identity
            || snapshot::resolve_commit(&root, "HEAD")? != head_now
        {
            issues.push(
                "source/index or HEAD changed during review; rerun on a stable snapshot".into(),
            );
        }
        issues.extend(
            verified
                .issues
                .into_iter()
                .map(|s| format!("verification: {s}")),
        );
    }
    issues.sort();
    issues.dedup();
    let unique_count = |predicate: &dyn Fn(&Consumer) -> bool| {
        consumers
            .iter()
            .filter(|c| c.still_present && predicate(c))
            .map(|c| &c.id)
            .collect::<BTreeSet<_>>()
            .len()
    };
    let impacted = unique_count(&|_| true);
    let direct = unique_count(&|c| c.depth == 1);
    let inferred = unique_count(&|c| c.evidence == "INFERRED");
    let tests = unique_count(&|c| c.is_test);
    let complete = issues.is_empty();
    let changed_count = declarations
        .iter()
        .map(|d| (&d.file, &d.id))
        .collect::<BTreeSet<_>>()
        .len();
    let score = complete.then(|| (changed_count.saturating_mul(2) + impacted).min(100) as u32);
    Ok(ReviewReport {
        schema_version: 1,
        score,
        level: score
            .map_or("unknown", |v| {
                if v < 30 {
                    "low"
                } else if v < 60 {
                    "medium"
                } else {
                    "high"
                }
            })
            .into(),
        scope: match scope {
            DiffScope::WorkingTree => "working-tree",
            DiffScope::Staged => "staged",
            DiffScope::Range { .. } => "commit-range",
        }
        .into(),
        base_commit: base,
        requested_base_commit: requested_base,
        head_commit: matches!(scope, DiffScope::Range { .. }).then_some(head),
        before_identity: before.identity,
        after_identity: after.identity,
        changed_files: changes,
        declarations,
        consumers,
        impacted,
        direct_consumers: direct,
        inferred_consumers: inferred,
        test_consumers: tests,
        coverage_complete: complete,
        coverage_issues: issues,
        unresolved_before: before.unresolved_edges,
        unresolved_after: after.unresolved_edges,
        files_indexed_before: before.files_indexed,
        files_indexed_after: after.files_indexed,
        indexing_ms: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
    })
}

pub fn render(report: &ReviewReport) -> String {
    let score = report
        .score
        .map(|s| format!("{s}/100 ({})", report.level))
        .unwrap_or_else(|| "unknown — incomplete coverage".into());
    let mut out = format!("# Change review\n\nCoverage: {}.\n\n{} changed file(s), {} declaration records, {} observed surviving consumers ({} direct, {} inferred, {} test consumers).\n\nBase: {}\nHead: {}\nScope: {}\n\n",
        if report.coverage_complete { "all supported source analyzed" } else { "incomplete — missing evidence is unknown, not zero impact" },
        report.changed_files.len(), report.declarations.len(), report.impacted, report.direct_consumers, report.inferred_consumers, report.test_consumers,
        report.base_commit, report.head_commit.as_deref().unwrap_or(&report.after_identity), report.scope);
    out.push_str("## Changed declarations\n\n");
    for declaration in &report.declarations {
        out.push_str(&format!(
            "- {} {} [{}] at {}:{}–{} ({})",
            declaration.change,
            declaration.label,
            declaration.snapshot,
            declaration.file,
            declaration.line,
            declaration.end_line,
            declaration.kind
        ));
        if !declaration.owners.is_empty() {
            out.push_str(&format!(
                " — owners {} ({})",
                declaration.owners.join(", "),
                declaration.owner_source.as_deref().unwrap_or("")
            ));
        }
        out.push('\n');
    }
    out.push_str("\n## Review and validation focus\n\n");
    for consumer in &report.consumers {
        out.push_str(&format!(
            "- {}{} at {}:{} [{}; {}; depth {}; {}] from {}",
            if consumer.is_test { "test: " } else { "" },
            consumer.label,
            consumer.file,
            consumer.line.unwrap_or(1),
            consumer.snapshot,
            consumer.evidence,
            consumer.depth,
            if consumer.still_present {
                "present after change"
            } else {
                "also removed"
            },
            consumer.changed_id
        ));
        if !consumer.owners.is_empty() {
            out.push_str(&format!(
                " — owners {} ({})",
                consumer.owners.join(", "),
                consumer.owner_source.as_deref().unwrap_or("")
            ));
        }
        out.push('\n');
        for step in &consumer.path {
            out.push_str(&format!(
                "  - {} → {}: {} {} at {}:{}\n",
                step.from,
                step.to,
                step.relation,
                step.evidence,
                step.file,
                step.line.unwrap_or(1)
            ));
        }
    }
    out.push_str("\n## Coverage\n\n");
    if report.coverage_complete {
        out.push_str("All supported source in both review snapshots was analyzed.\n");
    }
    for issue in &report.coverage_issues {
        out.push_str(&format!("- {issue}\n"));
    }
    out.push_str(&format!("\n{} / {} files indexed before/after in {} ms. Unresolved relationship targets: {} / {}.\n\nSecondary triage score: {score}. Formula: 2 per changed declaration plus 1 per surviving consumer, capped at 100.\n\nEvidence is structural reachability, not proof of runtime behavior. EXTRACTED/DECLARED = source facts; RESOLVED = unique name binding; INFERRED = heuristic. The weakest edge labels each complete chain. Source snapshots are rebuilt in memory and never overwrite the project graph.\n",
        report.files_indexed_before, report.files_indexed_after, report.indexing_ms, report.unresolved_before, report.unresolved_after));
    out
}
