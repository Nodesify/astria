"""Read-only extraction identity audit: exact scoped IDs, never fuzzy labels."""
import json
import pathlib
import sqlite3
import sys

root = pathlib.Path(sys.argv[1]).resolve()
graph = json.loads((root / ".astria/graph.json").read_text(encoding="utf-8"))
live = {node["id"] for node in graph["nodes"]}
connection = sqlite3.connect((root / ".astria/db.sqlite").as_uri() + "?mode=ro", uri=True)
extracted = {}
locations_by_id = {}
for (payload,) in connection.execute("SELECT nodes FROM extraction_cache"):
    for node in json.loads(payload):
        if node.get("node_type") in {"function", "test", "class", "method", "interface", "struct", "enum", "trait", "type", "constant", "variable", "module"}:
            extracted[node["id"]] = node
            locations_by_id.setdefault(node["id"], set()).add(
                (node.get("source_file"), node.get("source_line")))
rows = [{"id": key, "label": node.get("label"), "source_file": node.get("source_file"),
         "source_line": node.get("source_line"), "preserved": key in live}
        for key, node in sorted(extracted.items())]
duplicate_ids = [
    {"id": key, "locations": [
        {"source_file": source_file, "source_line": source_line}
        for source_file, source_line in sorted(locations, key=lambda location: (
            location[0] or "", -1 if location[1] is None else location[1]))]}
    for key, locations in sorted(locations_by_id.items()) if len(locations) > 1
]
print(json.dumps({"cached_code_definitions": len(rows), "preserved": sum(r["preserved"] for r in rows),
                  "definitions": rows, "duplicate_ids_with_distinct_locations": duplicate_ids}))
