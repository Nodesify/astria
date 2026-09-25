use std::path::Path;

use rusqlite::Connection;

pub fn export_graphml(db: &Connection, out_path: &Path) -> astria_core::Result<()> {
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<graphml xmlns=\"http://graphml.graphdrawing.org/xmlns\"\n");
    xml.push_str("         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"\n");
    xml.push_str("         xsi:schemaLocation=\"http://graphml.graphdrawing.org/xmlns\n");
    xml.push_str("         http://graphml.graphdrawing.org/xmlns/1.1/graphml.xsd\">\n");

    // Key definitions for node attributes
    xml.push_str("  <key id=\"label\" for=\"node\" attr.name=\"label\" attr.type=\"string\"/>\n");
    xml.push_str(
        "  <key id=\"file_type\" for=\"node\" attr.name=\"file_type\" attr.type=\"string\"/>\n",
    );
    xml.push_str(
        "  <key id=\"community\" for=\"node\" attr.name=\"community\" attr.type=\"int\"/>\n",
    );
    xml.push_str(
        "  <key id=\"source_file\" for=\"node\" attr.name=\"source_file\" attr.type=\"string\"/>\n",
    );
    xml.push_str(
        "  <key id=\"source_line\" for=\"node\" attr.name=\"source_line\" attr.type=\"int\"/>\n",
    );

    // Key definitions for edge attributes
    xml.push_str(
        "  <key id=\"relation\" for=\"edge\" attr.name=\"relation\" attr.type=\"string\"/>\n",
    );
    xml.push_str(
        "  <key id=\"confidence\" for=\"edge\" attr.name=\"confidence\" attr.type=\"string\"/>\n",
    );
    xml.push_str("  <key id=\"confidence_score\" for=\"edge\" attr.name=\"confidence_score\" attr.type=\"double\"/>\n");

    xml.push_str("  <graph id=\"G\" edgedefault=\"undirected\">\n");

    // Nodes
    {
        let mut stmt = db.prepare(
            "SELECT id, label, file_type, source_file, source_line, community FROM nodes",
        )?;
        #[allow(clippy::type_complexity)]
        let rows: Vec<(String, String, String, String, Option<i64>, Option<i64>)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        for (id, label, ft, sf, line, comm) in &rows {
            let esc_id = escape_xml(id);
            let esc_label = escape_xml(label);
            let esc_ft = escape_xml(ft);
            let esc_sf = escape_xml(sf);
            xml.push_str(&format!("    <node id=\"{}\">\n", esc_id));
            xml.push_str(&format!("      <data key=\"label\">{}</data>\n", esc_label));
            xml.push_str(&format!(
                "      <data key=\"file_type\">{}</data>\n",
                esc_ft
            ));
            xml.push_str(&format!(
                "      <data key=\"source_file\">{}</data>\n",
                esc_sf
            ));
            if let Some(l) = line {
                xml.push_str(&format!("      <data key=\"source_line\">{}</data>\n", l));
            }
            if let Some(c) = comm {
                xml.push_str(&format!("      <data key=\"community\">{}</data>\n", c));
            }
            xml.push_str("    </node>\n");
        }
    }

    // Edges
    {
        let mut stmt =
            db.prepare("SELECT source, target, relation, confidence, confidence_score FROM edges")?;
        let rows: Vec<(String, String, String, String, Option<f64>)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        for (src, tgt, rel, conf, score) in &rows {
            let esc_src = escape_xml(src);
            let esc_tgt = escape_xml(tgt);
            let esc_rel = escape_xml(rel);
            let esc_conf = escape_xml(conf);
            xml.push_str(&format!(
                "    <edge source=\"{}\" target=\"{}\">\n",
                esc_src, esc_tgt
            ));
            xml.push_str(&format!(
                "      <data key=\"relation\">{}</data>\n",
                esc_rel
            ));
            xml.push_str(&format!(
                "      <data key=\"confidence\">{}</data>\n",
                esc_conf
            ));
            if let Some(s) = score {
                xml.push_str(&format!(
                    "      <data key=\"confidence_score\">{}</data>\n",
                    s
                ));
            }
            xml.push_str("    </edge>\n");
        }
    }

    xml.push_str("  </graph>\n");
    xml.push_str("</graphml>\n");

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, xml)?;
    Ok(())
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
