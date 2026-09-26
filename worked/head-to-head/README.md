# Head-to-head: astria vs the original Python graphify

**Date:** 2026-09-25 · **Machine:** Windows 11 x64, both tools on the same machine.

> **Licensing & attribution:** Graphify is © Graphify Labs, dual-licensed Apache-2.0/MIT. The Graphify outputs stored under `worked/` exist to make these numbers verifiable. astria is an independent project — not affiliated with, sponsored by, or endorsed by Graphify Labs.

## What was compared

| | original graphify | astria |
|---|---|---|
| Version | commit `91f4d12` (pyproject v0.1.14) | 0.8.0 (prebuilt native binary) |
| Runtime | Python 3.12.12 + networkx/graspologic | Rust core + Node CLI (napi-rs) |
| Corpus | its own repository @ `91f4d12` (90 detect entries, ~0.5 MB text) | identical corpus |

Both tools ran the **structural pipeline only** (detect → AST extract → build → cluster → analyze → report), no LLM enrichment on either side. The original was driven by a script replicating its own `skill.md` stage-for-stage (`detect` → `extract` → `build_from_json` → `cluster`/`score_all` → `god_nodes`/`surprising_connections` → `generate`/`to_json`).

## Results (same corpus, structural-only)

| Metric | original graphify | astria |
|---|---|---|
| Build time (wall) | **21.92 s** (of which build+cluster+analyze: 20.30 s) | **4.68 s** |
| Nodes | 719 | 1,479 |
| Edges | 1,196 | 5,789 |
| Communities | 45 | 161 |
| Token benchmark (own methodology) | 50.1× (~1,738 tok/query) | 51.6× (~3,058 tok/query) |

Honest reading:

- **Speed:** ~4.7× faster end-to-end; the original spends 20.3 s of its 21.9 s in Python/networkx build+cluster.
- **Graph density:** ours extracts ~2× the nodes and ~4.8× the edges (we emit `Imports`/`Uses`/`Defines` in addition to calls, and aggregate file nodes). More relationships → finer communities (161 vs 45); the original's Leiden clustering merges more aggressively.
- **Token reduction:** effectively identical (50.1× vs 51.6×) — each tool measured with its **own** benchmark implementation (ours follows the same methodology). The comparable metric is each tool's ratio on the same corpus, not the absolute token estimates, because the corpus-size estimators differ (~87k vs ~158k tokens for the same files).
- **With local embeddings** (`--embed`, rewrite-only feature): 2,453 `similar_to` edges added, communities consolidate 161 → 91; ratio unchanged (51.5×) — embeddings add recall, not compression.
- **Parity + quality (new):** the snapshot workflow now also reports a `token_parity` block — both sides re-counted with one shared tokenizer (o200k_base), making absolute numbers directly comparable — and a golden-QA retrieval-quality harness (`scripts/bench/quality/`, recall@k / MRR over 35 grounded questions, run in CI as a non-blocking job) plus a blind promptfoo judging config and a LoCoMo memory-benchmark adapter (`scripts/bench/memory/`). First self-corpus quality numbers live in [`../astria/quality-results.json`](../astria/quality-results.json).

## Embedding experiment (v0.8.0, fresh runs)

| Corpus | without `--embed` | with `--embed` |
|---|---|---|
| this repo @ `44560ae` (191 files, 1.3 MB) | 2,063 nodes / 7,546 edges / **428** communities / 109.6× / 5.8 s | 2,063 nodes / 10,781 edges (**+3,235** `similar_to`) / **201** communities / 109.1× / +7.9 s |
| original graphify repo | 161 communities | 91 communities (**+2,453** `similar_to`) |

`similar_to` edges consolidate communities by ~44–53% on both corpora. The token ratio is unchanged: embeddings buy semantic recall and cleaner communities, not smaller output.

## Reproduce

```bash
# corpus
git clone https://github.com/safishamsi/graphify corpus && cd corpus && git checkout 91f4d12

# original (structural pipeline, as its skill.md drives it)
#   driver: detect -> extract -> build_from_json -> cluster -> analyze -> report (see results.json methodology)
#   then:   graphify benchmark graphify-out/graph.json

# ours
astria run .            # structural
astria run . --embed    # with local embeddings
```

Full machine-readable numbers: [`results.json`](./results.json). Historical (older-version) worked examples: [`../graphify-python/review.md`](../graphify-python/review.md), [`../astria/review.md`](../astria/review.md).

The same comparison is automated: `.github/workflows/bench-snapshot.yml`
(manual dispatch) runs both tools on a fresh GitHub runner via
`scripts/bench/orig_run.py` + `scripts/bench/run-snapshot.mjs`, commits the
result to `website/src/data/benchmarks-snapshot.json`, and the docs site
renders it live on the Benchmarks page.
