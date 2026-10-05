//! Deterministic community names derived from source modules, never hub symbols.
use rusqlite::Connection;
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub(super) fn source_labels(
    db: &Connection,
    ids: &[String],
    communities: &[u32],
    sources: &HashMap<String, (String, String)>,
) -> HashMap<u32, String> {
    let root = db
        .path()
        .and_then(|p| std::path::Path::new(p).parent()?.parent())
        .map(astria_paths::normalize);
    // Distinct files vote, so a generated file with thousands of symbols
    // cannot dominate the module name. Code wins over prose when present.
    let mut groups: BTreeMap<u32, BTreeMap<(bool, String), BTreeSet<String>>> = BTreeMap::new();
    for (id, &community) in ids.iter().zip(communities) {
        let candidates = groups.entry(community).or_default();
        let Some((file, kind)) = sources.get(id) else {
            continue;
        };
        if file.is_empty() || matches!(kind.as_str(), "stub" | "reference") {
            continue;
        }
        let normalized = astria_paths::normalize(std::path::Path::new(file));
        let relative = root
            .as_ref()
            .map(|r| astria_paths::relative_display(&normalized, r))
            .unwrap_or_else(|| normalized.clone());
        let mut parts: Vec<&str> = relative.split('/').filter(|p| !p.is_empty()).collect();
        let Some(filename) = parts.pop() else {
            continue;
        };
        // Workspace packages are the stable source module boundary. Within
        // a single package use its containing source directory instead.
        let module = if parts.len() >= 2 && matches!(parts[0], "crates" | "packages") {
            parts[..2].join("/")
        } else {
            while parts
                .last()
                .is_some_and(|p| matches!(*p, "src" | "source" | "lib"))
            {
                parts.pop();
            }
            if parts.is_empty() {
                filename
                    .rsplit_once('.')
                    .map(|(stem, _)| stem)
                    .unwrap_or(filename)
                    .to_string()
            } else {
                parts.join("/")
            }
        };
        candidates
            .entry((kind == "code", module))
            .or_default()
            .insert(normalized);
    }
    groups
        .into_iter()
        .map(|(community, candidates)| {
            let winner = candidates
                .iter()
                .max_by(|(ka, va), (kb, vb)| {
                    ka.0.cmp(&kb.0)
                        .then_with(|| va.len().cmp(&vb.len()))
                        .then_with(|| kb.1.cmp(&ka.1))
                })
                .map(|((_, module), _)| module.clone())
                .unwrap_or_else(|| format!("Community {community} (no source locus)"));
            (community, winner)
        })
        .collect()
}
