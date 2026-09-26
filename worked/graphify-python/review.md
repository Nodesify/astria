# Worked example: astria on the original Python graphify

> **Historical note.** Run with the pre-1.0 tool (then named
> `nodesify-graphify`, graph in `.graphify/`, ignore file `.graphifyignore`).
> The current binary is `astria` and the ignore file is `.astriaignore`.

**Corpus:** the Python project that inspired this tool, analyzed on its own codebase — 20 Python modules, tests, skill files, docs (see `CORPUS.md` for the exact commit and how to fetch it; not vendored, by policy).
**1,083 nodes / 6,823 edges / 84 communities**, modularity 0.509. Built with `run --embed --wiki`; benchmark: **40.2x** fewer tokens per query vs reading the corpus.

**Files:** `graph_report.md` (as generated), `wiki_index.md` (wiki entry point). The full `graph.json` (2.4 MB) is no longer stored in the repo — regenerate it with the command under **Reproduce**.

**Reproduce:** fetch the corpus per `CORPUS.md`, then `astria run . --embed --wiki` (as run pre-1.0: `nodesify-graphify run . --embed --wiki`).

## What the graph got right

- **Key Files is exactly right.** `extract.py` (2,501 edge endpoints), `analyze.py`, `export.py`, `serve.py`, `benchmark.py` — the five modules any maintainer would name first. Tests rank next, correctly separated from source.
- **`walk()` as top god node (degree 337) is true.** The tree-sitter AST walker really is the load-bearing function of the whole system; `_make_id()` and `add_node` beside it complete the real extraction core.
- **Module-level communities formed.** `graphify.wiki` (33 nodes), `validate.py` (22), `test_benchmark.py` (19) — coherent per-module clusters with sensible cohesion. 84 communities for 1,083 nodes is a healthier ratio than label propagation usually gets without similarity edges.
- **Cross-language semantic matches are real.** `observer ↔ watchdog.observers` (similar_to, from the C fixture to Python's watcher module) and `api/checks.py ↔ api.py` are genuine conceptual links the AST pass can't make — this is what the embedding layer is for.
- **The dominant "get" community (379 nodes, cohesion 0.70)** is honest: most of that codebase really is extraction plumbing that shares helpers.

## What the graph got wrong

- **Community labels are still symbol names.** "get", "exists", "walk()" name the biggest member, not the theme. "get" should be called something like "extraction-engine".
- **Test fixtures win community slots** (`sample.c`, 97 nodes — the multilanguage parser fixtures). Real files, but orientation noise; a `.graphifyignore` would remove them.
- **Duplicate-looking nodes**: `main()()` from a C fixture links to `main()` — per-file ids keep same-named symbols apart (correct), and the similarity layer then connects them (also correct), but the report surface makes this look odd.
- **Some INFERRED call edges to Python builtins** (`startswith`, `resolve`) inflate the `exists` community; stub exclusion dampens this but doesn't eliminate it for Python attribute-style calls.

## Verdict

On a mid-size Python codebase the graph nails files, hubs, and module structure — the orientation job. The 40.2x token figure is measured against the real corpus size. Weak spots are naming and fixture noise, same as the Rust self-example: consistent, known, and honest.
