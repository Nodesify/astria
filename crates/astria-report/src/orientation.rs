//! Source loci and orientation classes shared by report sections.
use rusqlite::Connection;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Orientation {
    Production,
    Documentation,
    Support,
}
impl Orientation {
    pub fn title(self) -> &'static str {
        match self {
            Self::Production => "Production Code",
            Self::Documentation => "Documentation",
            Self::Support => "Tests, Benchmarks and Examples",
        }
    }
}
pub(super) const ORIENTATIONS: [Orientation; 3] = [
    Orientation::Production,
    Orientation::Documentation,
    Orientation::Support,
];

pub(super) struct SourceNode {
    pub label: String,
    pub file: String,
    pub line: Option<i64>,
    pub community: Option<i64>,
    pub orientation: Orientation,
    pub degree: i64,
}

pub(super) struct Sources {
    pub nodes: HashMap<String, SourceNode>,
    root: Option<String>,
}
impl Sources {
    pub fn load(db: &Connection) -> astria_core::Result<Self> {
        let root = db
            .path()
            .and_then(|p| std::path::Path::new(p).parent()?.parent())
            .map(|p| p.to_string_lossy().replace('\\', "/"));
        let mut stmt = db.prepare(
            "WITH endpoints AS (SELECT source AS id FROM edges UNION ALL SELECT target FROM edges),
             degrees AS (SELECT id, COUNT(*) AS degree FROM endpoints GROUP BY id)
             SELECT n.id, n.label, n.source_file, n.source_line, n.community, n.file_type,
                    COALESCE(d.degree, 0)
             FROM nodes n LEFT JOIN degrees d ON n.id = d.id
             WHERE n.file_type NOT IN ('stub', 'reference') AND n.source_file != ''",
        )?;
        let rows = stmt.query_map([], |r| {
            let id = r.get::<_, String>(0)?;
            let file = r.get::<_, String>(2)?;
            let kind = r.get::<_, String>(5)?;
            let relative = root
                .as_ref()
                .map(|root| astria_paths::relative_display(&file, root))
                .unwrap_or_else(|| file.clone());
            Ok((
                id.clone(),
                SourceNode {
                    label: r.get(1)?,
                    orientation: classify(&relative, &kind, &id),
                    file,
                    line: r.get(3)?,
                    community: r.get(4)?,
                    degree: r.get(6)?,
                },
            ))
        })?;
        Ok(Self {
            nodes: rows.collect::<rusqlite::Result<_>>()?,
            root,
        })
    }
    pub fn link(&self, label: &str, file: &str, line: Option<i64>) -> String {
        let display = self
            .root
            .as_ref()
            .map(|r| astria_paths::relative_display(file, r))
            .unwrap_or_else(|| file.replace('\\', "/"));
        let path = if std::path::Path::new(&display).is_absolute() {
            display
        } else {
            format!("../{display}")
        };
        let mut url = String::new();
        for b in path.bytes() {
            if b.is_ascii_alphanumeric() || b"/-._~:".contains(&b) {
                url.push(b as char);
            } else {
                url.push_str(&format!("%{b:02X}"));
            }
        }
        if let Some(line) = line
            .filter(|line| *line >= 0)
            .and_then(|line| line.checked_add(1))
        {
            url.push_str(&format!("#L{line}"));
        }
        format!("[{}]({url})", escape(label))
    }
    pub fn node_link(&self, id: &str, label: &str) -> String {
        self.nodes
            .get(id)
            .map(|n| self.link(label, &n.file, n.line))
            .unwrap_or_else(|| escape(label))
    }
    pub fn display(&self, file: &str) -> String {
        self.root
            .as_ref()
            .map(|r| astria_paths::relative_display(file, r))
            .unwrap_or_else(|| file.replace('\\', "/"))
    }
}
pub(super) fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('*', "\\*")
        .replace('`', "\\`")
        .replace('\n', " ")
        .replace('\r', " ")
}
fn classify(file: &str, kind: &str, id: &str) -> Orientation {
    let normalized = file.replace('\\', "/").to_lowercase();
    let filename = normalized.rsplit('/').next().unwrap_or(&normalized);
    if kind == "test"
        || id.contains("::tests::")
        || id.ends_with("::tests")
        || normalized.split('/').any(|p| {
            matches!(
                p,
                "test"
                    | "tests"
                    | "__tests__"
                    | "spec"
                    | "specs"
                    | "bench"
                    | "benches"
                    | "benchmark"
                    | "benchmarks"
                    | "fixtures"
                    | "examples"
                    | "example"
            )
        })
        || filename.starts_with("test_")
        || filename.starts_with("bench_")
        || filename.ends_with("_test.go")
        || filename.ends_with("_test.py")
        || filename.contains(".test.")
        || filename.contains(".spec.")
    {
        Orientation::Support
    } else if kind != "code"
        || normalized
            .split('/')
            .any(|p| matches!(p, "docs" | "doc" | "documentation"))
    {
        Orientation::Documentation
    } else {
        Orientation::Production
    }
}
