---
sidebar_position: 2
title: Benchmarks and evidence
description: Measured token-reduction numbers, methodology, the head-to-head against the original Python Graphify, and the embedding experiment — all reproducible.
keywords: [benchmarks, token reduction, performance, methodology, evidence]
---

import BenchmarkSnapshot from '@site/src/components/BenchmarkSnapshot';

# Benchmarks and evidence

Every claim on this site is measured, printed after every run, and reproducible with the commands below. This page collects the canonical numbers (v0.8.0), the methodology behind them, and a head-to-head against the Python Graphify project that inspired it.

## How the token benchmark works

Every `run` and `update` prints a measured comparison:

- **Corpus side** — the real file sizes from the extraction manifest, converted with a fixed chars-per-token estimate. This is what a naive agent would read to answer questions.
- **Query side** — five fixed questions are run through the actual query engine and the answer text is counted. No sampling, no hand-picked best case.

The ratio is printed even when it is unflattering: on tiny corpora the benchmark honestly reports &lt;1× and says so — there the graph's value is structure, not compression.

The printed estimate uses a 4-chars-per-token heuristic. The [live snapshot](#live-benchmark-snapshot) additionally reports a `token_parity` block: both tools' corpus and query tokens re-counted with **one shared tokenizer** (o200k_base via js-tiktoken), so the snapshot's absolute numbers are directly comparable — the divergence between the two tools' own corpus estimates (~87k vs ~158k on the same files) is estimator, not bytes.

## Canonical numbers (v0.8.0)

Two corpora, fresh runs, same machine:

| Corpus | Files | Corpus tokens | Nodes / edges | Communities | Avg query cost | **Reduction** | Build time |
|---|---|---|---|---|---|---|---|
| this repository @ `44560ae` | 191 | ~333,000 | 2,063 / 7,546 | 428 | ~3,043 | **109.6×** | 5.8 s |
| original Python Graphify @ `91f4d12` | 90 entries | ~158,000 | 1,479 / 5,789 | 161 | ~3,058 | **51.6×** | 4.7 s |

Numbers vary per run and per corpus (file mix, repo size, and how chatty query answers are all matter). Older pages and release notes quote measurements from earlier versions and smaller file sets — for example 73–79× and 40.2× in the original worked examples. Treat those as dated records; the table above is canonical for v0.8.0.

## Head-to-head vs the original Python Graphify

The [original Graphify](https://github.com/safishamsi/graphify) (© Graphify Labs, dual-licensed Apache-2.0/MIT) is the Python project that inspired astria — an independent implementation, not affiliated with or endorsed by Graphify Labs. Both tools were run **on the same corpus** — the original's own repository at commit `91f4d12` — with the structural pipeline only (no LLM enrichment on either side), each driven the way its own documentation drives it.

| Metric | original Graphify (`91f4d12`) | astria 0.8.0 |
|---|---|---|
| Build time (wall) | 21.9 s (20.3 s of it build+cluster+analyze in Python/networkx) | 4.7 s |
| Nodes | 719 | 1,479 |
| Edges | 1,196 | 5,789 |
| Communities | 45 | 161 |
| Token benchmark (own methodology) | 50.1× (~1,738 tok/query) | 51.6× (~3,058 tok/query) |

Honest reading:

- **Speed**: ~4.7× faster end-to-end. The original spends most of its time in Python/networkx build and clustering; ours is a native Rust core with SQLite persistence.
- **Graph density**: ours extracts ~2× the nodes and ~4.8× the edges — `Imports`/`Uses`/`Defines` edges in addition to calls, plus file-aggregate nodes. That yields finer communities (161 vs 45); the original's Leiden clustering merges more aggressively. Denser is not automatically better — it is a different granularity trade-off.
- **Token reduction**: effectively identical (50.1× vs 51.6×). Each tool measured with its own benchmark implementation (ours follows the same methodology); the absolute corpus-token estimates differ (~87k vs ~158k) because the estimators differ, so the ratio — not the absolute tokens — is the comparable metric.

## Live benchmark snapshot

The table below is **regenerated automatically**: run **Benchmark snapshot → Run workflow** from the [Actions tab](https://github.com/Nodesify/astria/actions/workflows/bench-snapshot.yml), and the workflow runs both tools on a fresh GitHub runner, commits the updated snapshot JSON, and redeploys this site. This is the continuous proof that the numbers above stay honest.

<BenchmarkSnapshot />

CI runners are shared hardware, so treat snapshot numbers as trend data; the manual workstation run in the table above remains the detailed reference (it also includes the embedding experiment below).

## The embedding experiment

`--embed` adds a local embedding model (no API key, offline after a one-time ~90 MB download). Fresh off/on runs on v0.8.0:

| Corpus | without `--embed` | with `--embed` |
|---|---|---|
| this repository | 428 communities, 7,546 edges | **201** communities, 10,781 edges (**+3,235** `similar_to`) |
| original Graphify corpus | 161 communities | **91** communities (**+2,453** `similar_to`) |

Findings: `similar_to` edges consolidate communities by **44–53%** on both corpora, and the token ratio is unchanged (~109× / ~51.5×) — embeddings buy semantic recall and cleaner communities, not smaller output.

## Retrieval quality — not just compression

Cost says the graph is cheap; retrieval quality says whether it answers *well*. Three measurements live under `scripts/bench/`:

**Golden-QA recall** (`scripts/bench/quality/`) — 35 questions about this repository with ground-truth files, run through the real query engine and scored by the rank of the first expected file or symbol in the answer (recall@k, MRR). Expectations are validated against the tree (`--check`), so the set cannot silently rot. First structural (no-embeddings) measurement: recall@5 **8.6%**, recall@10 **17.1%**, MRR **0.094** — deliberately unflattering and useful: the miss mode was hub-ranked output and doc-dominated seeds. The fixes it pointed at shipped and were re-measured on the same set: answer nodes now rank by question-relevance first (then traversal distance, then degree), question words are filtered as stopwords, and code symbols outrank prose/stub nodes on equal term evidence. Current numbers: recall@1 **42.9%**, recall@5 **65.7%**, recall@10 **85.7%**, MRR **0.537** (35/35 answered, avg 0.19 s/query) — hybrid scoring, i.e. embedding seeds are active once `run --embed` has populated them and the model is cached. CI runs it on every snapshot dispatch (non-blocking while the golden set matures) and commits `website/src/data/quality-snapshot.json`.

**Blind LLM judging** (`scripts/bench/quality/promptfoo/`) — astria and the original Graphify answer the same golden questions; an LLM rubric grades each answer without knowing which tool produced it. Wired into the benchmark snapshot workflow as a gated step, and **deliberately restricted to the maintainer**: it runs only on a manual `Run workflow` dispatch by the maintainer account with `PROMPTFOO_JUDGE_KEY` set to an **OpenRouter** key (judge model: `openrouter:openai/gpt-4o-mini` by default — any OpenRouter model works, switch it in `promptfooconfig.yaml`), uploading the graded results as a workflow artifact. Automated runs and pull requests never call a paid API: fork PRs neither trigger this workflow nor receive secrets, and push-triggered snapshots skip judging.

**Memory retrieval — LoCoMo** (`scripts/bench/memory/`) — the evidence-referenced protocol the original publishes (snap-research LoCoMo, ACL 2024): ~2,000 QA pairs over 10 long conversations, ingested as `.astria/transcripts/` sidecars. `prepare-locomo.mjs` fetches and converts (1,977 pairs with resolvable evidence); `run-locomo.mjs` builds the graph and scores evidence-file recall@k / MRR, optionally with LLM-judged answer correctness. Dataset is CC BY-NC 4.0 — research use.

## Worked examples, including what went wrong

Two full runs with generated reports, graphs, and honest reviews of failure modes (unhelpful community labels, fixture noise, stub-noise connections):

- [The tool on itself](https://github.com/Nodesify/astria/tree/main/worked/astria) — 78.8× on the (then smaller) tree
- [The tool on the original Python Graphify](https://github.com/Nodesify/astria/tree/main/worked/graphify-python) — 40.2×, pinned commit
- [Head-to-head raw data](https://github.com/Nodesify/astria/tree/main/worked/head-to-head) — methodology, machine-readable `results.json`, the original tool's own benchmark output

## Reproduce

```bash
npm install -g @nodesify/astria

# self corpus
git clone https://github.com/Nodesify/astria && cd astria
astria run .            # prints the benchmark at the end
astria run . --embed    # embedding experiment

# Graphify's repository — the head-to-head corpus
git clone https://github.com/safishamsi/graphify corpus && cd corpus && git checkout 91f4d12
astria run .            # ours
# the original is driven per its skill.md: detect -> extract -> build -> cluster -> analyze -> report
# and measured with its own: graphify benchmark graphify-out/graph.json
```
