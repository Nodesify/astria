//! Read Git objects and local source without changing the checkout or publishing a graph.
use super::{ChangedFile, DiffScope};
use astria_core::{AstriaError, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_CORPUS_BYTES: usize = 256 * 1024 * 1024;

pub(super) enum Revision<'a> {
    Commit(&'a str),
    Index,
    WorkingTree,
}

pub(super) struct Sources {
    pub files: BTreeMap<String, Vec<u8>>,
    pub issues: Vec<String>,
    pub identity: String,
}

pub(super) fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()?;
    if !out.status.success() {
        return Err(AstriaError::Graph(format!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(out.stdout)
}

fn utf8(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| {
        AstriaError::Graph(
            "Git paths/output are not valid UTF-8; review coverage is unavailable".into(),
        )
    })
}

pub(super) fn resolve_commit(root: &Path, name: &str) -> Result<String> {
    if name.is_empty() || name.starts_with('-') {
        return Err(AstriaError::Graph(
            "empty or option-shaped Git revision".into(),
        ));
    }
    Ok(utf8(&git(
        root,
        &["rev-parse", "--verify", &format!("{name}^{{commit}}")],
    )?)?
    .trim()
    .to_owned())
}

pub(super) fn merge_base(root: &Path, base: &str, head: &str) -> Result<String> {
    Ok(utf8(&git(root, &["merge-base", base, head])?)?
        .trim()
        .to_owned())
}

fn source_candidate(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    astria_core::languages::for_extension(ext).is_some()
        || matches!(
            path,
            "CODEOWNERS" | ".github/CODEOWNERS" | "docs/CODEOWNERS" | ".astriaignore"
        )
}

pub(super) fn is_code(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    astria_core::languages::for_extension(ext).is_some()
}

pub(super) fn load(root: &Path, revision: Revision<'_>) -> Result<Sources> {
    let mut entries = Vec::new(); // path, Git mode, optional blob id
    match revision {
        Revision::Commit(commit) => {
            for record in git(root, &["ls-tree", "-r", "-z", "--full-tree", commit])?
                .split(|b| *b == 0)
                .filter(|r| !r.is_empty())
            {
                let (meta, path) = utf8(record)?
                    .split_once('\t')
                    .ok_or_else(|| AstriaError::Graph("invalid git tree record".into()))?;
                let fields: Vec<_> = meta.split_whitespace().collect();
                if fields.len() != 3 {
                    return Err(AstriaError::Graph("invalid git tree metadata".into()));
                }
                if source_candidate(path) || fields[1] == "commit" {
                    entries.push((
                        path.to_owned(),
                        fields[0].to_owned(),
                        Some(fields[2].to_owned()),
                    ));
                }
            }
        }
        Revision::Index => {
            for record in git(root, &["ls-files", "--stage", "-z"])?
                .split(|b| *b == 0)
                .filter(|r| !r.is_empty())
            {
                let (meta, path) = utf8(record)?
                    .split_once('\t')
                    .ok_or_else(|| AstriaError::Graph("invalid index record".into()))?;
                let fields: Vec<_> = meta.split_whitespace().collect();
                if fields.len() != 3 || fields[2] != "0" {
                    return Err(AstriaError::Graph(format!("unmerged index entry: {path}")));
                }
                if source_candidate(path) || fields[0] == "160000" {
                    entries.push((
                        path.to_owned(),
                        fields[0].to_owned(),
                        Some(fields[1].to_owned()),
                    ));
                }
            }
        }
        Revision::WorkingTree => {
            // Submodules have no language extension but may contain consumers.
            // Preserve that coverage gap for the working tree as for commits.
            for record in git(root, &["ls-files", "--stage", "-z"])?
                .split(|b| *b == 0)
                .filter(|r| !r.is_empty())
            {
                let (meta, path) = utf8(record)?
                    .split_once('\t')
                    .ok_or_else(|| AstriaError::Graph("invalid index record".into()))?;
                let fields: Vec<_> = meta.split_whitespace().collect();
                if fields.len() != 3 || fields[2] != "0" {
                    return Err(AstriaError::Graph(format!("unmerged index entry: {path}")));
                }
                if fields[0] == "160000" {
                    entries.push((path.to_owned(), "160000".into(), None));
                }
            }
            for record in git(
                root,
                &[
                    "ls-files",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                    "-z",
                ],
            )?
            .split(|b| *b == 0)
            .filter(|r| !r.is_empty())
            {
                let path = utf8(record)?;
                if source_candidate(path) {
                    entries.push((path.to_owned(), "working".into(), None));
                }
            }
        }
    }
    entries.sort();
    entries.dedup();
    let mut sources = Sources {
        files: BTreeMap::new(),
        issues: Vec::new(),
        identity: String::new(),
    };
    let mut object_entries = Vec::new();
    let mut working_bytes = 0usize;
    for (path, mode, oid) in entries {
        if mode == "120000" || mode == "160000" {
            sources.issues.push(format!(
                "{path}: symlink/submodule is outside structural review coverage"
            ));
        } else if let Some(oid) = oid {
            object_entries.push((path, oid));
        } else {
            // A missing tracked path is a legitimate worktree deletion. Other
            // I/O errors are coverage gaps, never silently successful empties.
            let absolute = root.join(&path);
            match std::fs::symlink_metadata(&absolute) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => sources.issues.push(format!("{path}: {e}")),
                Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => sources
                    .issues
                    .push(format!("{path}: not a regular source file")),
                Ok(meta) if meta.len() > MAX_FILE_BYTES as u64 => sources
                    .issues
                    .push(format!("{path}: exceeds 2 MiB review limit")),
                Ok(meta) => {
                    if working_bytes.saturating_add(meta.len() as usize) > MAX_CORPUS_BYTES {
                        sources
                            .issues
                            .push(format!("{path}: exceeds 256 MiB review corpus limit"));
                        continue;
                    }
                    match absolute.canonicalize() {
                        Ok(resolved) if resolved.starts_with(root) => {
                            match std::fs::read(&resolved) {
                                Ok(bytes) if bytes.len() <= MAX_FILE_BYTES => {
                                    working_bytes += bytes.len();
                                    sources.files.insert(path, bytes);
                                }
                                Ok(_) => sources
                                    .issues
                                    .push(format!("{path}: grew beyond review source-size limit")),
                                Err(e) => sources.issues.push(format!("{path}: {e}")),
                            }
                        }
                        Ok(_) => sources
                            .issues
                            .push(format!("{path}: source resolves outside repository")),
                        Err(e) => sources.issues.push(format!("{path}: {e}")),
                    }
                }
            }
        }
    }
    if !object_entries.is_empty() {
        read_objects(root, &object_entries, &mut sources)?;
    }
    let mut total = 0;
    sources.files.retain(|path, bytes| {
        total += bytes.len();
        if total > MAX_CORPUS_BYTES {
            sources
                .issues
                .push(format!("{path}: exceeds 256 MiB review corpus limit"));
            false
        } else {
            true
        }
    });
    let mut hasher = Sha256::new();
    for (path, bytes) in &sources.files {
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    sources.identity = format!("sha256:{:x}", hasher.finalize());
    Ok(sources)
}

fn read_objects(root: &Path, entries: &[(String, String)], sources: &mut Sources) -> Result<()> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| AstriaError::Graph("Git batch stdin unavailable".into()))?;
    let input = entries
        .iter()
        .map(|(_, id)| format!("{id}\n"))
        .collect::<String>();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let mut reader = BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| AstriaError::Graph("Git batch stdout unavailable".into()))?,
    );
    let read_result = (|| -> Result<()> {
        let mut total = 0usize;
        for (path, oid) in entries {
            let mut header = String::new();
            reader.read_line(&mut header)?;
            let parts: Vec<_> = header.split_whitespace().collect();
            if parts.len() != 3 || parts[0] != oid || parts[1] != "blob" {
                return Err(AstriaError::Graph(format!(
                    "cannot read source blob for {path}: {}",
                    header.trim()
                )));
            }
            let size = parts[2]
                .parse::<u64>()
                .map_err(|_| AstriaError::Graph("invalid Git object size".into()))?;
            if size > MAX_FILE_BYTES as u64
                || total.saturating_add(size as usize) > MAX_CORPUS_BYTES
            {
                std::io::copy(&mut reader.by_ref().take(size), &mut std::io::sink())?;
                sources
                    .issues
                    .push(format!("{path}: exceeds review source-size limit"));
            } else {
                let mut bytes = vec![0; size as usize];
                reader.read_exact(&mut bytes)?;
                total += bytes.len();
                sources.files.insert(path.clone(), bytes);
            }
            let mut newline = [0];
            reader.read_exact(&mut newline)?;
            if newline != *b"\n" {
                return Err(AstriaError::Graph("invalid Git batch separator".into()));
            }
        }
        Ok(())
    })();
    if read_result.is_err() {
        let _ = child.kill();
    }
    // Only this exact child can be terminated; never process-name/category kills.
    drop(reader);
    let write_result = writer
        .join()
        .map_err(|_| AstriaError::Graph("Git batch writer failed".into()))?;
    let status = child.wait()?;
    read_result?;
    write_result?;
    if !status.success() {
        return Err(AstriaError::Graph("Git batch source read failed".into()));
    }
    Ok(())
}

fn diff_args<'a>(scope: &DiffScope, base: &'a str, head: &'a str) -> Vec<&'a str> {
    match scope {
        DiffScope::Range { .. } => vec![base, head],
        DiffScope::Staged => vec!["--cached", base],
        DiffScope::WorkingTree => vec![base],
    }
}

pub(super) fn changes(
    root: &Path,
    scope: &DiffScope,
    base: &str,
    head: &str,
) -> Result<Vec<ChangedFile>> {
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--name-status",
        "-z",
        "--find-renames",
    ];
    args.extend(diff_args(scope, base, head));
    args.push("--");
    let output = git(root, &args)?;
    let fields = output
        .split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .map(utf8)
        .collect::<Result<Vec<_>>>()?;
    let mut changed = Vec::new();
    let mut i = 0;
    while i < fields.len() {
        let status = fields[i];
        i += 1;
        let path = *fields
            .get(i)
            .ok_or_else(|| AstriaError::Graph("missing diff path".into()))?;
        i += 1;
        let rename = status.starts_with('R') || status.starts_with('C');
        let next = if rename {
            let p = *fields
                .get(i)
                .ok_or_else(|| AstriaError::Graph("missing rename target".into()))?;
            i += 1;
            p
        } else {
            path
        };
        changed.push(ChangedFile {
            status: status.to_owned(),
            old_path: (!status.starts_with('A')).then(|| path.to_owned()),
            new_path: (!status.starts_with('D')).then(|| next.to_owned()),
        });
    }
    if matches!(scope, DiffScope::WorkingTree) {
        for path in git(root, &["ls-files", "--others", "--exclude-standard", "-z"])?
            .split(|b| *b == 0)
            .filter(|r| !r.is_empty())
        {
            changed.push(ChangedFile {
                status: "untracked".into(),
                old_path: None,
                new_path: Some(utf8(path)?.to_owned()),
            });
        }
    }
    Ok(changed)
}

type LineRanges = Vec<(u32, u32)>;

pub(super) fn hunks(
    root: &Path,
    scope: &DiffScope,
    base: &str,
    head: &str,
    change: &ChangedFile,
) -> Result<(LineRanges, LineRanges)> {
    // Added, deleted, renamed and untracked paths select complete declarations;
    // there is no need to ask Git to render their potentially binary contents.
    if change.old_path != change.new_path || change.status == "untracked" {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--no-renames",
        "--unified=0",
    ];
    args.extend(diff_args(scope, base, head));
    args.push("--");
    if let Some(path) = &change.old_path {
        args.push(path);
    }
    if let Some(path) = &change.new_path {
        if Some(path) != change.old_path.as_ref() {
            args.push(path);
        }
    }
    let patch = git(root, &args)?;
    let pattern = regex::Regex::new(r"(?m)^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")
        .expect("literal hunk regex");
    let mut old = Vec::new();
    let mut new = Vec::new();
    for cap in pattern.captures_iter(&String::from_utf8_lossy(&patch)) {
        for (out, start, count) in [(&mut old, 1, 2), (&mut new, 3, 4)] {
            let line = cap[start]
                .parse::<u32>()
                .map_err(|_| AstriaError::Graph("invalid hunk line".into()))?;
            let len = cap
                .get(count)
                .map_or(Ok(1), |v| v.as_str().parse::<u32>())
                .map_err(|_| AstriaError::Graph("invalid hunk count".into()))?;
            if len > 0 {
                out.push((line, line.saturating_add(len - 1)));
            }
        }
    }
    Ok((old, new))
}
