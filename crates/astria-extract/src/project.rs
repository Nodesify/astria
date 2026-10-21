//! Project-aware import resolution: the import specifiers a project's own
//! metadata defines — TypeScript `paths` aliases with `baseUrl`, workspace
//! package source entries, and their subpaths — mapped to authored source
//! files instead of bare-name coincidence. Anything the metadata does not
//! cover (unresolved aliases, ambiguous entries, node_modules packages)
//! stays exactly as unresolved as before; this layer only adds mappings
//! the project itself declared.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Longest-prefix alias mappings plus workspace package entries, loaded
/// once per extraction run from the project root.
#[derive(Debug, Default, Clone)]
pub struct ImportContext {
    /// The scanned project root every mapping is relative to.
    root: PathBuf,
    /// Alias patterns ("@db" or "@/*") to root-relative target patterns,
    /// longest wildcard prefix first. Exact aliases take precedence.
    /// From tsconfig/jsconfig `compilerOptions.paths`, relative to `baseUrl`.
    aliases: Vec<(String, String)>,
    /// Workspace package names ("@scope/pkg", "lib") to their authored
    /// entry source file (absolute, when it exists on disk).
    workspace_entries: HashMap<String, PathBuf>,
}

impl ImportContext {
    pub fn is_empty(&self) -> bool {
        self.aliases.is_empty() && self.workspace_entries.is_empty()
    }

    /// Map an import specifier to an authored source path the extraction
    /// corpus can key on. Returns the path with extension candidates not
    /// yet resolved — the caller (module lookup) matches against extracted
    /// files, which is the ground truth for what a path resolves to.
    pub fn resolve(&self, specifier: &str) -> Option<PathBuf> {
        if self.root.as_os_str().is_empty() {
            return None;
        }
        if let Some(mapped) = self.resolve_alias(&self.root, specifier) {
            return Some(mapped);
        }
        self.resolve_workspace(specifier)
    }

    /// The module-lookup key for a project-mapped specifier: the resolved
    /// path in the same normalized form extraction indexes files by.
    pub fn module_key(&self, specifier: &str) -> Option<String> {
        self.resolve(specifier)
            .map(|path| path.to_string_lossy().replace("\\", "/"))
    }

    fn resolve_alias(&self, root: &Path, specifier: &str) -> Option<PathBuf> {
        if let Some((_, target)) = self
            .aliases
            .iter()
            .find(|(alias, _)| !alias.contains('*') && alias == specifier)
        {
            return Some(root.join(target));
        }
        for (pattern, target) in &self.aliases {
            let Some((prefix, suffix)) = pattern.split_once('*') else {
                continue;
            };
            if let Some(capture) = specifier
                .strip_prefix(prefix)
                .and_then(|rest| rest.strip_suffix(suffix))
            {
                return Some(root.join(target.replace('*', capture)));
            }
        }
        None
    }

    fn resolve_workspace(&self, specifier: &str) -> Option<PathBuf> {
        // Exact package name, or a subpath of it. Scoped names contain a
        // slash themselves ("@acme/lib/sub"), so match the longest known
        // package prefix rather than splitting on the first '/'.
        let mut best: Option<(&str, &PathBuf)> = None;
        for (name, entry) in &self.workspace_entries {
            let is_match = specifier == name.as_str() || specifier.starts_with(&format!("{name}/"));
            if !is_match {
                continue;
            }
            if best.is_none_or(|(current, _)| name.len() > current.len()) {
                best = Some((name.as_str(), entry));
            }
        }
        let (name, entry) = best?;
        let subpath = specifier[name.len()..].trim_start_matches('/');
        if subpath.is_empty() {
            return Some(entry.clone());
        }
        let dir = entry.parent()?;
        let joined = dir.join(normalize_separators(subpath));
        // Directory-shaped subpaths stay directory-shaped: module lookup
        // already tries index files for directories.
        Some(joined)
    }
}

fn normalize_separators(path: &str) -> String {
    path.replace('\\', "/")
}

/// Read `compilerOptions.baseUrl` and `compilerOptions.paths` from
/// tsconfig.json / jsconfig.json. `paths` values are arrays; only the
/// first target of each alias is used (multiple targets are ambiguous
/// project intent — one mapping is better than guessing).
fn load_tsconfig_aliases(root: &Path) -> Vec<(String, String)> {
    let mut aliases = Vec::new();
    for name in ["tsconfig.json", "jsconfig.json"] {
        let Ok(text) = std::fs::read_to_string(root.join(name)) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let options = value.get("compilerOptions");
        let base = options
            .and_then(|o| o.get("baseUrl"))
            .and_then(|b| b.as_str())
            .map(normalize_separators)
            .unwrap_or_else(|| String::from("."));
        let Some(paths) = options
            .and_then(|o| o.get("paths"))
            .and_then(|p| p.as_object())
        else {
            continue;
        };
        for (alias, targets) in paths {
            let Some(first) = targets.as_array().and_then(|a| a.first()) else {
                continue;
            };
            let Some(target) = first.as_str() else {
                continue;
            };
            // Keep exact file targets and wildcard positions intact. TypeScript
            // permits one wildcard per pattern, including a suffix after it.
            if alias.is_empty()
                || target.is_empty()
                || alias.matches('*').count() > 1
                || target.matches('*').count() > 1
                || (!alias.contains('*') && target.contains('*'))
            {
                continue;
            }
            let target = Path::new(&base).join(normalize_separators(target));
            aliases.push((
                alias.clone(),
                normalize_separators(&target.to_string_lossy()),
            ));
        }
        // First config file that declares paths wins; a second one would
        // describe a different compilation unit.
        if !aliases.is_empty() {
            break;
        }
    }
    // Longest alias prefix first so "@app/*" outruns "@/*".
    aliases
        .sort_by_key(|(pattern, _)| std::cmp::Reverse(pattern.find('*').unwrap_or(pattern.len())));
    aliases.dedup();
    aliases
}

/// Workspace member package names to their authored entry file. Entries
/// resolve only when the pointed-to file exists with a source extension —
/// a `main` of `dist/index.js` (build output) is deliberately not mapped
/// when the authored `src/index.ts` sits next to it.
fn load_workspace_entries(root: &Path) -> HashMap<String, PathBuf> {
    let mut entries = HashMap::new();
    let Ok(text) = std::fs::read_to_string(root.join("package.json")) else {
        return entries;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return entries;
    };
    let mut patterns: Vec<String> = Vec::new();
    for workspaces in [value.get("workspaces"), value.get("workspacePackages")] {
        if let Some(list) = workspaces.and_then(|w| w.as_array()) {
            for item in list.iter().filter_map(|i| i.as_str()) {
                patterns.push(normalize_separators(item));
            }
        }
    }
    // npm/yarn/pnpm workspace arrays may nest ("packages" object form).
    if let Some(fields) = value.get("workspaces").and_then(|w| w.as_object()) {
        for list in fields.values().filter_map(|v| v.as_array()) {
            for item in list.iter().filter_map(|i| i.as_str()) {
                patterns.push(normalize_separators(item));
            }
        }
    }
    for pattern in patterns {
        // Only the common "dir/*" pattern is expanded; anything else
        // (explicit lists are handled by direct member dirs) is skipped.
        let Some(prefix) = pattern.strip_suffix('*') else {
            continue;
        };
        let Some(dir) = strip_prefix_path(root, prefix) else {
            continue;
        };
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.filter_map(|e| e.ok()) {
            let member = entry.path().join("package.json");
            let Ok(member_text) = std::fs::read_to_string(&member) else {
                continue;
            };
            let Ok(member_value) = serde_json::from_str::<serde_json::Value>(&member_text) else {
                continue;
            };
            let Some(name) = member_value.get("name").and_then(|n| n.as_str()) else {
                continue;
            };
            for field in ["source", "types", "main", "module"] {
                let Some(pointed) = member_value.get(field).and_then(|m| m.as_str()) else {
                    continue;
                };
                let candidate = entry.path().join(normalize_separators(pointed));
                // Authored TypeScript source only: a `main` pointing at
                // dist/build output is compilation, not source; JS-only
                // entries keep the pre-existing bare-name resolution.
                let resolved = source_entry(&candidate).or_else(|| {
                    candidate
                        .extension()
                        .is_none()
                        .then(|| {
                            ["ts", "mts", "tsx"]
                                .iter()
                                .find_map(|ext| source_entry(&candidate.with_extension(ext)))
                        })
                        .flatten()
                });
                if let Some(resolved) = resolved {
                    entries.insert(name.to_string(), resolved);
                    break;
                }
            }
        }
    }
    entries
}

/// The pointed entry, or its index sibling when it names a directory.
fn source_entry(candidate: &Path) -> Option<PathBuf> {
    if candidate.is_file() {
        // Only authored TypeScript source counts (see the caller note).
        return matches!(
            candidate.extension().and_then(|e| e.to_str()),
            Some("ts" | "mts" | "tsx")
        )
        .then(|| candidate.to_path_buf());
    }
    if candidate.is_dir() {
        for index in ["index.ts", "index.mts", "index.tsx"] {
            let sibling = candidate.join(index);
            if sibling.is_file() {
                return Some(sibling);
            }
        }
    }
    None
}

fn strip_prefix_path(root: &Path, prefix: &str) -> Option<PathBuf> {
    let trimmed = prefix.trim_matches('/');
    if trimmed.is_empty() {
        return Some(root.to_path_buf());
    }
    let joined = root.join(normalize_separators(trimmed));
    Some(joined)
}

/// Load the project import context for a scanned root. Missing or partial
/// metadata yields an empty/partial context — never an error, and never a
/// guess beyond what the project itself declared.
pub fn load_import_context(root: &Path) -> ImportContext {
    ImportContext {
        root: root.to_path_buf(),
        aliases: load_tsconfig_aliases(root),
        workspace_entries: load_workspace_entries(root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn tsconfig_paths_alias_resolves_to_source_root() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("tsconfig.json"),
            r#"{"compilerOptions": {"baseUrl": ".", "paths": {"@/*": ["src/*"], "@app/*": ["lib/app/*"]}}}"#,
        );
        let ctx = load_import_context(dir.path());
        let resolved = ctx
            .resolve("@/components/button")
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        assert!(resolved.ends_with("/src/components/button"), "{resolved}");
        // Longest prefix wins: @app/* outruns @/* even when both match.
        let resolved = ctx
            .resolve("@app/theme")
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        assert!(resolved.ends_with("/lib/app/theme"), "{resolved}");
        // Non-alias specifiers are untouched (None: caller keeps its logic).
        assert!(ctx.resolve("react").is_none());
    }

    #[test]
    fn workspace_entries_map_names_and_subpaths() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        );
        write(
            &dir.path().join("packages/lib/package.json"),
            r#"{"name": "@acme/lib", "main": "./src/index.ts"}"#,
        );
        write(
            &dir.path().join("packages/lib/src/index.ts"),
            "export const x = 1;\n",
        );
        let ctx = load_import_context(dir.path());
        let entry = ctx.workspace_entries.get("@acme/lib").unwrap();
        assert!(entry
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("src/index.ts"));
        let sub = ctx.resolve("@acme/lib/util/format").unwrap();
        assert!(
            sub.to_string_lossy()
                .replace('\\', "/")
                .ends_with("src/util/format"),
            "{}",
            sub.display()
        );
        // Unknown packages stay unresolved.
        assert!(ctx.resolve("unknown-pkg").is_none());
    }

    #[test]
    fn build_outputs_are_not_treated_as_authored_source() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        );
        write(
            &dir.path().join("packages/built/package.json"),
            // main points at compiled output with no authored sibling.
            r#"{"name": "built", "main": "./dist/index.js"}"#,
        );
        write(
            &dir.path().join("packages/built/dist/index.js"),
            "compiled\n",
        );
        let ctx = load_import_context(dir.path());
        assert!(
            !ctx.workspace_entries.contains_key("built"),
            "dist-only entries must not map"
        );
    }

    #[test]
    fn empty_context_is_inert() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = load_import_context(dir.path());
        assert!(ctx.is_empty());
        assert!(ctx.resolve("@/x").is_none());
    }
}
