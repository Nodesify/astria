// Filesystem-hierarchy tree export: a self-contained collapsible HTML tree
// of every graph node grouped by source file path, with per-directory
// descendant counts and a hover inspector showing each symbol's top edges.
// Ported from upstream astria v8 tree_html.py (vanilla JS instead of D3,
// so no vendored asset is needed).

use std::collections::HashMap;
use std::path::Path;

use rusqlite::Connection;

use astria_core::Result;

const TOP_K_EDGES: usize = 5;

#[derive(serde::Serialize)]
struct TreeNode {
    name: String,
    /// descendant symbol count (directories only)
    count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_type: Option<String>,
    /// top outbound edges for the hover inspector: [relation, target_label]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    edges: Vec<[String; 2]>,
    children: Vec<TreeNode>,
}

fn sort_tree(node: &mut TreeNode) {
    node.children.sort_by(|a, b| a.name.cmp(&b.name));
    for child in &mut node.children {
        sort_tree(child);
    }
}

/// Build the filesystem hierarchy from node source paths and render a
/// self-contained HTML page. Returns the number of symbols in the tree.
pub fn export_tree(db: &Connection, out_path: &Path, max_children: usize) -> Result<usize> {
    // Adjacency for the hover inspector: node id -> top outbound edges
    let mut outbound: HashMap<String, Vec<[String; 2]>> = HashMap::new();
    {
        let mut stmt = db.prepare(
            "SELECT e.source, e.relation, t.label
             FROM edges e JOIN nodes t ON t.id = e.target
             ORDER BY e.source",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows.flatten() {
            let entry = outbound.entry(row.0).or_default();
            if entry.len() < TOP_K_EDGES {
                entry.push([row.1, row.2]);
            }
        }
    }

    let mut dirs: HashMap<String, Vec<TreeNode>> = HashMap::new();
    let mut symbol_count = 0usize;

    {
        let mut stmt =
            db.prepare("SELECT id, label, file_type, source_file FROM nodes ORDER BY label")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for (id, label, file_type, source_file) in rows.flatten() {
            let path = source_file.replace('\\', "/");
            let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
            if parts.is_empty() {
                continue;
            }
            let dir_key = parts[..parts.len() - 1].join("/");
            let leaf = TreeNode {
                name: parts[parts.len() - 1].to_string(),
                count: 0,
                id: Some(id.clone()),
                label: Some(label),
                file_type: Some(file_type),
                edges: outbound.get(&id).cloned().unwrap_or_default(),
                children: Vec::new(),
            };
            dirs.entry(dir_key).or_default().push(leaf);
            symbol_count += 1;
        }
    }

    // Nest directory keys into a tree: each directory's children are its
    // symbol leaves plus its immediate subdirectories (built recursively).
    fn build_dir(
        path: &str,
        leaves: &mut HashMap<String, Vec<TreeNode>>,
        max_children: usize,
    ) -> TreeNode {
        let dir_name = if path.is_empty() {
            "root".to_string()
        } else {
            path.rsplit('/').next().unwrap_or(path).to_string()
        };
        let mut children = leaves.remove(path).unwrap_or_default();
        children.sort_by(|a, b| a.name.cmp(&b.name));
        if children.len() > max_children {
            let extra = children.len() - max_children;
            children.truncate(max_children);
            children.push(TreeNode {
                name: format!("(+{extra} more)"),
                count: 1,
                id: None,
                label: None,
                file_type: None,
                edges: Vec::new(),
                children: Vec::new(),
            });
        }
        let prefix = if path.is_empty() {
            String::new()
        } else {
            format!("{path}/")
        };
        let mut subdirs: Vec<String> = leaves
            .keys()
            .filter(|k| {
                if prefix.is_empty() {
                    !k.contains('/')
                } else {
                    k.strip_prefix(&prefix)
                        .map(|rest| !rest.contains('/'))
                        .unwrap_or(false)
                }
            })
            .cloned()
            .collect();
        subdirs.sort();
        for sub in subdirs {
            children.push(build_dir(&sub, leaves, max_children));
        }
        TreeNode {
            name: dir_name,
            count: 0,
            id: None,
            label: None,
            file_type: None,
            edges: Vec::new(),
            children,
        }
    }

    // Materialize intermediate directories that contain no files directly:
    // real graphs use deep absolute paths, and a branch like "//?/C:/repo"
    // would otherwise never appear because nothing sits inside it at depth 1.
    let all_keys: Vec<String> = dirs.keys().cloned().collect();
    for key in all_keys {
        let mut prefix = key.clone();
        while let Some(pos) = prefix.rfind('/') {
            prefix.truncate(pos);
            if !prefix.is_empty() && !dirs.contains_key(&prefix) {
                dirs.insert(prefix.clone(), Vec::new());
            }
        }
    }

    let mut root = build_dir("", &mut dirs, max_children);
    sort_tree(&mut root);
    update_counts(&mut root);

    let data = serde_json::to_string(&root)?;
    let html = render_html(&data, symbol_count);
    std::fs::write(out_path, html)?;
    Ok(symbol_count)
}

fn update_counts(node: &mut TreeNode) {
    if node.id.is_some() {
        node.count = 1;
        return;
    }
    for child in &mut node.children {
        update_counts(child);
    }
    node.count = node.children.iter().map(|c| c.count).sum();
}

fn render_html(data: &str, symbol_count: usize) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>astria tree — {symbol_count} symbols</title>
<style>
  body {{ font-family: ui-monospace, Consolas, monospace; margin: 0; display: flex; }}
  #tree {{ overflow: auto; height: 100vh; padding: 12px; flex: 1; }}
  ul {{ list-style: none; padding-left: 18px; margin: 0; }}
  li {{ margin: 1px 0; white-space: nowrap; }}
  .dir {{ cursor: pointer; color: #b48ead; font-weight: 600; }}
  .dir::before {{ content: "▸ "; color: #666; }}
  .dir.open::before {{ content: "▾ "; }}
  .file {{ color: #81a1c1; }}
  .sym {{ color: #d8dee9; cursor: help; border-bottom: 1px dotted #4c566a; }}
  .more {{ color: #666; font-style: italic; }}
  .count {{ color: #666; font-size: 0.85em; margin-left: 4px; }}
  #inspector {{ width: 320px; border-left: 1px solid #333; padding: 12px;
               height: 100vh; overflow: auto; background: #1c1c1c; color: #d8dee9; }}
  #inspector h3 {{ margin-top: 0; color: #88c0d0; }}
  .edge {{ font-size: 0.85em; color: #a3be8c; }}
</style>
</head>
<body>
<div id="tree"></div>
<div id="inspector"><h3>Inspector</h3><p id="insp-body">Hover a symbol.</p></div>
<script>
const DATA = {data};
const treeEl = document.getElementById('tree');
const inspEl = document.getElementById('insp-body');

function nodeRow(n) {{
  if (n.id) {{
    const li = document.createElement('li');
    li.className = 'sym';
    li.textContent = n.label + '  ·  ' + n.name;
    li.onmouseenter = () => inspect(n);
    return li;
  }}
  const li = document.createElement('li');
  const dir = document.createElement('span');
  dir.className = 'dir open';
  dir.textContent = n.name;
  const count = document.createElement('span');
  count.className = 'count';
  count.textContent = '(' + n.count + ')';
  li.appendChild(dir); li.appendChild(count);
  const ul = document.createElement('ul');
  for (const c of n.children) ul.appendChild(nodeRow(c));
  li.appendChild(ul);
  dir.onclick = () => {{
    dir.classList.toggle('open');
    ul.style.display = ul.style.display === 'none' ? '' : 'none';
  }};
  return li;
}}

function inspect(n) {{
  let html = '<b>' + n.label + '</b><br><small>' + n.name + ' · ' + (n.file_type || '') + '</small>';
  if (n.edges && n.edges.length) {{
    html += '<br><br>Top connections:';
    for (const [rel, target] of n.edges) html += '<div class="edge">--' + rel + '--&gt; ' + target + '</div>';
  }} else {{
    html += '<br><br><small>(no edges)</small>';
  }}
  inspEl.innerHTML = html;
}}

const rootUl = document.createElement('ul');
for (const c of DATA.children) rootUl.appendChild(nodeRow(c));
treeEl.appendChild(rootUl);
</script>
</body>
</html>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use astria_core::db::open_db_in_memory;

    #[test]
    fn exports_tree_with_counts() {
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file) VALUES
              ('a', 'Alpha()', 'code', 'src/core/a.rs'),
              ('b', 'Beta()', 'code', 'src/core/b.rs'),
              ('c', 'Gamma()', 'code', 'src/c.py');
            INSERT INTO edges (source, target, relation, confidence, source_file)
              VALUES ('a', 'b', 'calls', 'EXTRACTED', 'src/core/a.rs');
        ",
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("tree.html");
        let count = export_tree(&db, &out, 40).unwrap();
        assert_eq!(count, 3);
        let html = std::fs::read_to_string(&out).unwrap();
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Alpha()"));
        assert!(html.contains("calls"));
        assert!(html.contains(r#""edges":[["calls","Beta()"]]"#));
    }

    #[test]
    fn nested_only_tree_renders_parents() {
        // No file sits directly in a parent dir - intermediate dirs must
        // still materialize (the browser caught this on real absolute paths)
        let db = open_db_in_memory().unwrap();
        db.execute_batch(
            "
            INSERT INTO nodes (id, label, file_type, source_file) VALUES
              ('a', 'Alpha()', 'code', '?/C:/repo/crates/core/src/a.rs'),
              ('b', 'Beta()', 'code', '?/C:/repo/crates/core/src/b.rs');
        ",
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("tree.html");
        let count = export_tree(&db, &out, 40).unwrap();
        assert_eq!(count, 2);
        let html = std::fs::read_to_string(&out).unwrap();
        // The data must contain nested path segments, i.e. root has children
        assert!(html.contains("crates"));
        assert!(html.contains("core"));
    }

    #[test]
    fn max_children_truncates() {
        let db = open_db_in_memory().unwrap();
        for i in 0..10 {
            db.execute(
                "INSERT INTO nodes (id, label, file_type, source_file) VALUES (?1, ?2, 'code', ?3)",
                rusqlite::params![format!("n{i}"), format!("N{i}()"), format!("x/file_{i}.rs")],
            )
            .unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("tree.html");
        let count = export_tree(&db, &out, 3).unwrap();
        assert_eq!(count, 10);
        let html = std::fs::read_to_string(&out).unwrap();
        assert!(html.contains("(+7 more)"));
    }
}
