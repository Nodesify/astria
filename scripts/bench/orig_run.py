"""Run the ORIGINAL Python graphify's structural pipeline exactly as its
skill.md drives it (Part A AST + Part C merge + Step 4), skipping Part B
(LLM semantic subagents) so both tools get the same no-LLM workload.

Writes a machine-readable results JSON to the path given as argv[2].

Usage: python orig_run.py <corpus_dir> <out_json>
"""
import json
import sys
import time
from pathlib import Path

corpus = Path(sys.argv[1]).resolve()
out_path = Path(sys.argv[2]).resolve()
t0 = time.perf_counter()

from graphify.detect import detect
from graphify.extract import collect_files, extract
from graphify.build import build_from_json
from graphify.cluster import cluster, score_all
from graphify.analyze import god_nodes, surprising_connections, suggest_questions
from graphify.report import generate
from graphify.export import to_json

# Step 2 - detect
result = detect(corpus)
(corpus / ".graphify_detect.json").write_text(json.dumps(result, indent=2, default=str))
t_detect = time.perf_counter() - t0
files = result.get("files", {})
files_detected = sum(len(v) for v in files.values())

# Part A - AST extraction
code_files = []
for f in files.get("code", []):
    p = Path(f)
    code_files.extend(collect_files(p) if p.is_dir() else [p])
t1 = time.perf_counter()
ast = extract(code_files)
t_extract = time.perf_counter() - t1

# Part C - merge (AST only; Part B LLM skipped on both sides)
merged = {
    "nodes": ast["nodes"],
    "edges": ast["edges"],
    "input_tokens": 0,
    "output_tokens": 0,
}
(corpus / ".graphify_extract.json").write_text(json.dumps(merged, indent=2, default=str))

# Step 4 - build, cluster, analyze, report
t2 = time.perf_counter()
extraction = json.loads((corpus / ".graphify_extract.json").read_text())
detection = json.loads((corpus / ".graphify_detect.json").read_text())

G = build_from_json(extraction)
communities = cluster(G)
cohesion = score_all(G, communities)
tokens = {"input": 0, "output": 0}
gods = god_nodes(G)
surprises = surprising_connections(G, communities)
labels = {cid: "Community " + str(cid) for cid in communities}
questions = suggest_questions(G, communities, labels)

out = corpus / "graphify-out"
out.mkdir(exist_ok=True)
report = generate(G, communities, cohesion, labels, gods, surprises, detection, tokens, str(corpus), suggested_questions=questions)
(out / "GRAPH_REPORT.md").write_text(report)
to_json(G, communities, str(out / "graph.json"))
t_build = time.perf_counter() - t2
t_total = time.perf_counter() - t0

# Graph stats from the tool's own export
g = json.loads((out / "graph.json").read_text())
comm_ids = set()
for n in g.get("nodes", []):
    c = n.get("community")
    if isinstance(c, list):
        comm_ids.update(c)
    elif c is not None:
        comm_ids.add(c)

data = {
    "build_seconds": round(t_total, 2),
    "stages": {
        "detect": round(t_detect, 2),
        "extract": round(t_extract, 2),
        "build_cluster_analyze_report": round(t_build, 2),
    },
    "files_detected": files_detected,
    "nodes": len(g.get("nodes", [])),
    "edges": len(g.get("links", [])),
    "communities": len(comm_ids),
}
out_path.write_text(json.dumps(data, indent=2))
print(json.dumps(data))
