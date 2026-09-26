# Benchmarks

Everything measured about this project lives in this directory. Public
numbers and methodology:
[Benchmarks and evidence](https://nodesify.github.io/astria/docs/explanation/benchmarks).

| Path | What it measures | Where it runs |
|---|---|---|
| `run-snapshot.mjs` + `tokenize.mjs` | Head-to-head vs the original Python Graphify on the same corpus: build time, graph shape, `token_parity` (both tools' corpus and query tokens counted with ONE shared o200k_base tokenizer — absolute numbers directly comparable) | CI (`bench-snapshot` workflow, manual dispatch) |
| `orig_run.py` | Drives the original Python Graphify through its own structural pipeline, stage for stage, so both tools do identical work | Same CI job |
| `quality/` | Retrieval quality: 35 grounded golden questions through the real query engine, scored by recall@k / MRR of the files and symbols answers surface (`--check` validates expectations against the tree); plus a blind promptfoo config where an LLM rubric judges astria vs the original without knowing which tool answered | CI non-blocking job + local |
| `memory/` | LoCoMo long-conversation memory benchmark (~2,000 QA pairs over 10 conversations, ingested as transcript sidecars): evidence-file recall@k / MRR, optional LLM-judged answer correctness | Local |

Each subdirectory's README has the exact commands. Ground rules shared by
all of it: measure on the same corpus, count with the same tokenizer, report
unflattering numbers, and keep the golden set honest (expectations fail the
build when they stop matching the tree).
