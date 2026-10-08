use serde_json::{json, Value};

pub(super) fn output_schema() -> Value {
    json!({"type": "object", "required": ["text", "nodeCount", "edgeCount", "nodes", "edges", "nextCursor", "graphGeneration", "freshness"],
    "properties": {
        "text": {"type": "string"}, "nodeCount": {"type": "integer"}, "edgeCount": {"type": "integer"},
        "nextCursor": {"type": ["integer", "null"]}, "graphGeneration": {"type": ["string", "null"]},
        "graphBuiltAt": {"type": ["string", "null"]},
        "nodes": {"type": "array", "items": {"type": "object", "required": ["id", "label", "fileType", "sourceFile", "sourceLine"], "properties": {
            "id": {"type": "string"}, "label": {"type": "string"}, "fileType": {"type": "string"},
            "sourceFile": {"type": "string"}, "sourceLine": {"type": ["integer", "null"]},
            "community": {"type": ["integer", "null"]}, "signature": {"type": ["string", "null"]}, "summary": {"type": ["string", "null"]}
        }}},
        "edges": {"type": "array", "items": {"type": "object", "required": ["source", "target", "relation", "confidence", "sourceFile", "sourceLine"], "properties": {
            "source": {"type": "string"}, "target": {"type": "string"}, "relation": {"type": "string"},
            "confidence": {"type": "string"}, "confidenceScore": {"type": ["number", "null"]},
            "sourceFile": {"type": "string"}, "sourceLine": {"type": ["integer", "null"]}
        }}},
        "freshness": {"type": ["object", "null"], "properties": {
            "status": {"type": "string"}, "added": {"type": "integer"}, "modified": {"type": "integer"},
            "deleted": {"type": "integer"}, "filesChecked": {"type": "integer"},
            "extractionOutdated": {"type": "boolean"}, "artifactsChecked": {"type": "boolean"},
            "artifactsConsistent": {"type": ["boolean", "null"]}, "graphGeneration": {"type": ["string", "null"]},
            "graphBuiltAt": {"type": ["string", "null"]}, "staleExternalIndexes": {"type": "array", "items": {"type": "string"}},
            "checkMilliseconds": {"type": "integer"}, "error": {"type": ["string", "null"]}
        }},
        "renderedTokens": {"type": "integer"}, "elapsedMilliseconds": {"type": "integer"}, "snapshotEstimatedBytes": {"type": "integer"}
    }})
}
