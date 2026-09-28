---
sidebar_position: 2
title: Benchmarks and evidence
description: Measured token-reduction numbers, methodology, the head-to-head against the original Python Graphify, and the embedding experiment — all reproducible.
keywords: [benchmarks, token reduction, performance, methodology, evidence]
---

import BenchmarkSnapshot from '@site/src/components/BenchmarkSnapshot';

# Benchmarks and evidence

For the latest source validation, see [September 2026 retrieval results](./retrieval-validation.md), including symbol diagnostics and performance tradeoffs.

The tables below are historical measurements with the methodology and limitations recorded here. This page collects the canonical numbers (v0.8.0), the methodology behind them, and a head-to-head against the Python Graphify project that inspired it.

## How the token benchmark works

Every `run` and `update` prints a measured comparison:

- **Corpus side** — the real file sizes from the extraction manifest, converted with a fixed chars-per-token estimate. This is a full-corpus size reference; agents typically use targeted search and read only selected files.
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

The table below is **regenerated automatically**: run **Benchmark snapshot → Run workflow** from the [Actions tab](https://github.com/Nodesify/astria/actions/workflows/bench-snapshot.yml), and the workflow runs both tools on a fresh GitHub runner, commits the updated snapshot JSON, and redeploys this site. The snapshot measures the installed release; proposed-source quality is checked separately.

<BenchmarkSnapshot />

CI runners are shared hardware, so treat snapshot numbers as trend data; the manual workstation run in the table above remains the detailed reference (it also includes the embedding experiment below).

## The embedding experiment

`--embed` adds a local embedding model (no API key, offline after a one-time ~90 MB download). Fresh off/on runs on v0.8.0:

| Corpus | without `--embed` | with `--embed` |
|---|---|---|
| this repository | 428 communities, 7,546 edges | **201** communities, 10,781 edges (**+3,235** `similar_to`) |
| original Graphify corpus | 161 communities | **91** communities (**+2,453** `similar_to`) |

Findings: `similar_to` edges consolidate communities by **44–53%** on both corpora, and the token ratio is unchanged (~109× / ~51.5×) — this experiment shows different graph structure and similar output size; it does not establish a retrieval-quality improvement.

## Retrieval quality — not just compression

Cost says the graph is cheap; retrieval quality says whether it answers *well*. Three measurements live under `scripts/bench/`:

**Repository retrieval (schema v2)** (`scripts/bench/quality/`) reports exact-file hit@k, true recall@k across all expected files, and MRR. All questions remain in the denominator, including failed queries. Historical scores labeled “recall” measured first file-or-symbol hits and omitted errors; they are not schema v2 measurements. See [September 2026 retrieval results](./retrieval-validation.md) for the latest schema-compatible paired measurements.

**External comparison** (scripts/bench/external/) pins Click, Express and ripgrep to immutable commits and supplies source-grounded questions. The paired runner compares Astria with the original Graphify at 1000- and 4000-token budgets using the same o200k_base tokenizer and clipping rules. See [September 2026 retrieval results](./retrieval-validation.md) for published results and limitations; these small sets do not establish broad reasoning quality.

Full-corpus/query ratios measure context size, not actual agent token savings. The targeted baseline measures retrieval output only; graph construction, search scan cost and end-to-end task completion are outside its scope. See [the harness methodology](https://github.com/Nodesify/astria/tree/main/scripts/bench/quality) for reproduction and limitations.

**Source quality gate** (`quality.yml`) builds the native module and CLI from each proposed checkout and requires self-corpus recall@5 of at least 50%. Reports record CLI version, checkout commit and dirty state, corpus revision, golden hash and query settings. The installed-release snapshot is a separate workflow and cannot validate proposed changes.

**Blind LLM judging** (`scripts/bench/quality/promptfoo/`) — astria and the original Graphify answer the same golden questions; an LLM rubric grades each answer without knowing which tool produced it. Wired into the benchmark snapshot workflow as a gated step, and **deliberately restricted to the maintainer**: it runs only on a manual `Run workflow` dispatch by the maintainer account with the `OPENROUTER_API_KEY` secret set to an **OpenRouter** key (judge model: any OpenRouter model — set the repo variable `JUDGE_MODEL` under Settings → Actions → Variables, e.g. `openrouter:anthropic/claude-3.5-haiku`; default `openai/gpt-4o-mini`), uploading the graded results as a workflow artifact. Automated runs and pull requests never call a paid API: fork PRs neither trigger this workflow nor receive secrets, and push-triggered snapshots skip judging.

First verdicts landed September 28 via the deterministic blind judge `scripts/bench/quality/blind-judge.mjs` (TypeSafe System One, `jev-latest`): on the 35 self-corpus questions, astria answers graded 100% PASS against Graphify's 77.1% PASS (mean score 1.92 vs 1.61 of 2). The same answer pairs re-graded the same day by the organizationally independent promptfoo/OpenRouter judge (`gpt-4o-mini`, strict rubric) put astria at 77.1% pass vs Graphify 65.7% — a narrower but agreeing ordering, stricter on both tools. Single judge, single run each — not statistical claims. Details and limitations: [retrieval validation](./retrieval-validation.md#blind-answer-correctness-judging-typesafe-september-28). The key was funded again in September 2026; a maintainer dispatch can re-run the independent grader at any time.

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
