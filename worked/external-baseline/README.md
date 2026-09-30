# External baseline: astria vs targeted rg + source reads

**Date:** 2026-09-30 · **Machine:** Windows 11 x64, Ryzen AI 9 HX 370, Node 24.15.0, rg 14.1.0 · **Candidate:** CLI 1.0.7, local build of `b0a9c2e` whose only compiled-in difference was the in-progress `astria-embed` model swap (structural runs never load the embedder). The native-artifact SHA-256 recorded in every run matches the benchmarked binary byte-for-byte, pinning it against later working-tree changes; full per-run provenance is in `results.json`.

> Corpora are upstream projects cloned at pinned commits by the runner (never installed or executed): Click 8.1.8 (`934813e`), Express 4.21.2 (`1faf228`), ripgrep 14.1.1 (`4649aa9`), each under its own license. This directory stores astria's measurement of its own retrieval over them.

## The question this answers

Every earlier comparison measured astria against Graphify, and full-corpus/query token ratios are explicitly size diagnostics. Neither answers "what does a graph buy over just searching?" This is the first checked-in measurement of astria against a search-based alternative on the same corpora at the same delivered-token budgets.

## Method

`node scripts/bench/external/run.mjs` from the repository root: fresh clones of the pinned corpora, one structural graph per corpus (`astria run .`, no LLM, no embeddings), then the eight golden questions through `scripts/bench/quality/run-quality.mjs` at requested budgets of 1,000 and 4,000 tokens for both methods. The 250- and 500-token points extend the curve by invoking `run-quality.mjs` directly over the same graphs.

- **astria:** `astria query <question> --budget <b> --depth 3`. Depth 3 is the harness default (the external runner passes no `--depth`); the paired Graphify protocol used depth 2, so these numbers are not interchangeable with that page's.
- **Baseline (`question-rg-plus-source-windows`):** deterministic and question-derived only — lowercase word terms from the question, `rg -i -m 3` per term, files ranked by term occurrences over the matched lines (path order breaks ties), a 10-before/30-after line window read around each file's first match, files accumulated until the budget is reached. Golden paths and symbols never enter the search.
- **Scoring:** both methods pass through the same `o200k_base` complete-line clipping before file-rank parsing; failed, empty and nonzero-exit responses stay in the denominator. Delivered tokens count retrieval context only — not filesystem bytes scanned, graph construction, or model reasoning.

## Results (8 questions: 3 Click, 2 Express, 3 ripgrep)

Aggregated across corpora, one observation per condition:

| Budget (tokens) | astria MRR | astria hit@1 | astria hit@5 | astria avg delivered | rg MRR | rg hit@1 | rg hit@5 | rg avg delivered |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 250 | 1.000 | 100% | 100% | 207 | 0.000 | 0% | 0% | 236 |
| 500 | 1.000 | 100% | 100% | 474 | 0.000 | 0% | 0% | 493 |
| 1,000 | 1.000 | 100% | 100% | 970 | 0.031 | 0% | 12.5% | 994 |
| 4,000 | 1.000 | 100% | 100% | 3,971 | 0.092 | 0% | 25% | 3,992 |

- astria ranked the defining file **first for 8/8 questions at every budget**. At the 250-token budget its responses were 184–234 delivered tokens each (total 1,652 for the eight questions) with zero clipping — the engine's internal accounting ended every response inside budget without the harness clipping anything.
- The baseline **never ranked the defining file first at any budget**. Its best ranks at 4,000 tokens were 4, 5, 9, 11, 12; three questions (click1, express1, ripgrep1) missed entirely at every budget. Every baseline response was clipped. More context did not rescue it: extra budget added distractor files (tests, docs) that outranked the definitions.
- Latency, for completeness: astria averaged 0.37–0.42 s per query; the baseline 0.09–0.22 s.

## Honest reading

- **What this establishes:** first-shot navigation quality per delivered token, against a deterministic single-pass search. For these eight questions, ~207 delivered tokens of graph answer put the defining file first every time; ~4,000 tokens of single-pass search results and source windows never did. Relative to this baseline that is roughly **19× less delivered context for strictly better ranking** (1,652 vs ~31,900 tokens across the set, 8/8 vs 0/8 rank-1).
- **What it does not establish:** the baseline is not an expert iterative searcher — it is a floor, as the harness itself states ("deterministic, not a claim about expert iterative search"). A skilled agent refining queries and reading targeted files would do better than this floor, and that comparison is unmeasured here. The eight questions are symbol-heavy and name their target symbols — astria's home turf; doc-intent and multi-hop behavior are measured on the paired tracks. One observation per condition, no variance. Graph construction cost (one `astria run .` per corpus, seconds; the baseline has none) is excluded from token accounting — for a single question the build dominates, across a session it amortizes.

## Reproduce

```sh
npm install --no-save js-tiktoken     # once, at the repository root
node scripts/build-native.mjs         # dist/astria.node from this tree
(cd packages/astria-cli && npm run build)
node scripts/bench/external/run.mjs   # 1000/4000 × both methods; needs a fresh bench-work/external
# curve extension (per corpus):
node scripts/bench/quality/run-quality.mjs --root bench-work/external/click \
  --golden scripts/bench/external/click.jsonl --astria packages/astria-cli/dist/index.js \
  --budget 250 --out scripts/bench/quality/out/click-250-astria.json
# add --baseline for the rg arm; repeat per corpus and budget
```

The runner refuses a corpus directory that already contains a graph; use a fresh `bench-work/external` for another end-to-end run. `results.json` beside this file holds all 24 runs (3 corpora × 4 budgets × 2 methods) with per-item ranks, token counts, clipping flags and full provenance.
