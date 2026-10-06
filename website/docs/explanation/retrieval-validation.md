---
title: Retrieval validation (September–October 2026)
description: Paired local retrieval measurements, symbol preservation and performance tradeoffs.
sidebar_position: 3
---

# Retrieval validation (September–October 2026)

Latest paired run: **October 6, 2026 — astria 1.1.0 vs Graphify 0.9.77** (first section below). The September 27 run compared the 1.0.6 tree at `cbe8364` (chunked document retrieval, cross-conversation ranking, chunk overlap, introspection commands) against Graphify v0.9.69 pinned to `4139885a1212956cf69a76946fbde0d181ab85e9`. The September 30 targeted-search baseline section below ran separately on CLI 1.0.7 from `b0a9c2e`. Machine in both paired runs: Windows, Ryzen AI 9 HX 370, Node 24, Python 3.12.12. Each tool receives fresh archives of the same pinned corpora. Structural extraction only, no LLMs or embeddings, BFS depth 2, exact shared `o200k_base` counting and identical complete-line clipping for scoring. Raw budget overruns are counted before clipping. Each condition has one observation including process startup; timing differences are not statistically established.

## Paired run: astria 1.1.0 vs Graphify 0.9.77 (October 6)

Same protocol and pinned external corpora as the September runs; the self corpus refreshed to the current develop tree (`5e69c63cee176a7addc3b5c4414280bba3d2d815`). Graphify moved 174 commits upstream of the September pin to **v0.9.77** (`5c7b84792f453582676548185aaec3824d51dfe2`, editable install; the venv pins it and the runner verifies the commit). Astria 1.1.0, release profile, build and queries using the identical native binary. Splits: frozen v1, corrected v2, additional-validation v1 — the reserved splits were not used. Raw measurements retained in `bench-work/paired-1.1.0-vs-g0977-20261006` (gitignored), config and full report alongside.

File recall@5 / MRR at the 1,000-token budget (the 4,000 budget scores within a point everywhere; self at 4,000: astria 78.6% / 0.586, Graphify 55.7% / 0.464):

| Corpus / split | Questions | astria | Graphify |
|---|---:|---:|---:|
| Astria self (frozen v1) | 35 | **75.7% / 0.570** | 55.7% / 0.464 |
| Astria self (corrected v2) | 35 | **75.7% / 0.570** | 56.7% / 0.467 |
| Click (frozen v1) | 3 | **100% / 1.000** | 100% / 0.667 |
| Click heldout v1 | 2 | **100% / 0.750** | 50% / 0.500 |
| Express (frozen v1) | 2 | **100% / 1.000** | 100% / 0.625 |
| Express heldout v1 | 2 | **100% / 0.667** | 50% / 0.500 |
| ripgrep (frozen v1) | 3 | 100% / 0.778 | 100% / **0.833** |
| ripgrep heldout v1 | 3 | **100% / 0.667** | 33.3% / 0.333 |

Definition recall@5 (exact declaration inside the top five returned nodes): Click **60% vs 0%**, Click heldout **100% vs 0%**, Express **100% vs 50%**, Express heldout 100% vs 100%, ripgrep heldout **50% vs 0%**. Graphify's misses are concentrated on same-name ambiguity (`invoke` ×4 on Click) — its grounded `Context.invoke` and `BaseCommand.invoke` definitions are present in its graph but never returned.

Budget compliance across all 170 responses per tool: **astria 0 over budget**; Graphify 59 over at 1,000 tokens and 50 over at 4,000 (its self answers averaged ~5,954 raw tokens against the 4,000 request). Mean query latency 0.49 s (astria) vs 0.58 s (Graphify). Symbol preservation stays at 100% of extraction-cache IDs on every corpus (2,642 self, 1,148 Click, 3,263 Express, 3,091 ripgrep), and every grounded definition is present at its implementation line (Click 5/5, Click heldout 2/2, Express 2/2, Express heldout 1/1, ripgrep heldout 4/4).

Extraction and build (single observation each): astria's graph is denser on every corpus — self 6,356 nodes / 22,212 edges vs 3,566 / 7,465; Click 2,278 / 6,177 vs 1,835 / 3,516; Express 4,414 / 12,098 vs **489** / 837; ripgrep 5,107 / 12,965 vs 4,049 / 8,403. Build time flipped on these small corpora: Graphify 0.9.77 is faster on 7 of 8 corpus/split conditions across four repositories (self 13.6 s vs 15.8 s, ripgrep 8.3 s vs 11.9 s) and markedly faster than the 0.9.69 measured in September; astria is faster only on the frozen Click split (5.6 s vs 6.5 s). Graphify is faster on the additional Click validation split (5.2 s vs 6.0 s). Build pipelines differ, so this is practical elapsed time on small inputs, not an algorithm benchmark — the September large-corpus comparison measured the opposite ordering.

Honest reading: astria's numbers replicate the 1.0.6 run (self recall@5 78.6% at 4,000 tokens in both; MRR 0.586 vs 0.618). Graphify's self recall@5 reads 55.7–56.7% here versus 65.7% in September — a 174-commit upstream delta and run-to-run variance are both in play (its own September runs moved 65.7% ↔ 67.1%), so the drop is recorded, not attributed. The ordering agrees with every prior paired measurement, and the small external sets still do not establish general superiority.

## Frozen file retrieval

Original questions, recall among the first five distinct returned files, at 4,000 tokens:

| Corpus | Questions | 1.0.5 run | 1.0.6 candidate | Graphify |
|---|---:|---:|---:|---:|
| Astria self | 35 | 77.1% | 78.6% | 65.7% |
| Click | 3 | 66.7% | 100% | 100% |
| Express | 2 | 0% | 100% | 100% |
| ripgrep | 3 | 100% | 100% | 100% |

Self MRR is 0.618 (0.674 in the 1.0.5 run) and hit@5 holds at 82.9%: chunked prose now competes for rankings, and the chunk prior in code-majority graphs recovers most but not all of the earlier ranking precision. Graphify self recall@5 reads 65.7% in the same 1.0.6 run (67.1% in its own earlier run); run-to-run variance on its side is real, and astria leads under both readings. Seven separately authored validation questions achieved recall@5 of 100%/100%/66.7% for Click/Express/ripgrep, versus 50%/50%/33.3% for Graphify. These cases have been exercised during development and are not untouched held-out evidence. The small external sets do not establish general superiority.

## Correctness diagnostics

### Failure-mode priorities

Ranked evaluation priorities for retrieval changes, derived from the measured weaknesses below. Any ranking or corpus change must be evaluated against all three, reported side by side — the paired runner's `report.mjs` table now emits the required trio (definition recall@5 + definition MRR, file recall@5 + file MRR, average delivered tokens with over-budget counts) plus a per-case miss list for triage:

1. **Ambiguous same-name symbols.** Unqualified prose over colliding bare names (`invoke` x4 on Click) still ranks the right definition outside the top five. Definition-recall metrics, not file recall, are the acceptance bar here — a file hit with the wrong symbol is a silent failure.
2. **Prose displacing code results.** Chunked document prose competes with code symbols in rankings and occasionally hijacks seeds (the two ripgrep `command`-helper lexical hijacks). Code-intent questions must not regress when document ranking improves.
3. **Small external datasets.** Every external corpus is tiny (2–5 questions per split) and all non-reserved sets have been exercised; no result generalizes. Growing genuinely held-out corpora (the reserved splits stay reserved until their first evaluation run) matters more than further paired re-runs of the same questions.

All query processes succeeded across the eight corpus/split builds. Astria stayed within budget for every response at both budgets (Graphify exceeded its own 1,000-token budget on 29/35 self questions). Every extraction-cache definition ID survived publication (1,432/1,432 self, 1,147/1,147 Click, 3,263/3,263 Express, 3,076/3,076 ripgrep). All known Click/Express/ripgrep-heldout definitions were present at their implementation lines (5/5, 2/2+1/1, 4/4).

Exact-symbol top-five recall is weaker than file recall, and the September 28 ranking fixes narrowed the gap on the 1.0.6 pinned graphs: the additional Click cases moved from 0% to 2/2 definitions surfaced (the missing `BaseCommand.get_usage` had lost a seed slot to a same-name label tie and now reserves one via its qualified scope), the additional ripgrep cases from 25% to 3/4 (the remaining miss ranks 7th), and the self code set's MRR rose from 0.618 (paired run) to 0.687 at unchanged recall. Two same-name `invoke` implementations on Click still land at ranks 6 and 9 — genuinely ambiguous unqualified prose over four same-name symbols.

The validation surface also grew: `scripts/bench/paired/*.heldout-v2.jsonl` adds 14 separately authored cases (5 Click, 5 Express, 4 ripgrep) grounded in the pinned corpora with line-exact definitions. On the current runtime they retrieve at 5/5, 5/5, and 2/4 files respectively (the two ripgrep misses are lexical hijacks by `command`-helper chunks), with 11/13 v2 definitions inside the top five. These cases were exercised during verification and are not untouched held-out evidence.

## Budget-response curve (September 28)

The paired runner's budgets are configurable (`budgets` array in the config; default 1,000/4,000). A four-point curve (250/500/1000/2000) ran over the same seven corpus/split sets as the frozen runs — 50 questions × 4 budgets × 2 tools, same pins, same tokenizer, same clipping rules (`bench-work/budget-curve-20260928`). Aggregated across all sets:

| Budget (tokens) | astria MRR | astria recall@5 | Graphify MRR | Graphify recall@5 | Graphify raw over budget |
|---:|---:|---:|---:|---:|---:|
| 250 | 0.680 | 74% | 0.522 | 66% | 48/50 |
| 500 | 0.717 | 84% | 0.530 | 69% | 48/50 |
| 1,000 | 0.722 | 85% | 0.531 | 69% | 41/50 |
| 2,000 | 0.725 | 85% | 0.531 | 69% | 11/50 |

The headline: **astria's 250-token answers outscore Graphify's 2,000-token answers on every metric** (0.680 vs 0.531 MRR; 74% vs 69% recall@5) — an 8× budget advantage at equal quality, with the curve nearly flat above 500 tokens (MRR 0.717 → 0.725). Astria stayed inside the requested budget on all 200 responses; Graphify exceeded the 250- and 500-token budgets on 48 of 50 raw responses at each budget (its counted answers average ~1.5× the request at 250 and ~1.2× at 500), so under a hard token constraint Graphify cannot actually deliver what it retrieves. Structural only, one observation per condition — a compliance-and-cost result, not a statistical claim. The reserved goldens were not used in this run.

## Blind answer-correctness judging (TypeSafe, September 28)

The blind judging blocked since the 1.0.4 comparison — every OpenRouter judge call failed on an exhausted key — ran with a TypeSafe System One judge (`jev-latest`, score-style graded verdicts) via `scripts/bench/quality/blind-judge.mjs`. Both tools answered the same 35 rubric-grounded questions on the self corpus, and the judge graded each answer against the golden rubric without knowing which tool produced it:

| Tool | PASS | PARTIAL | FAIL | mean score (0–2) | mean judge confidence |
|---|---:|---:|---:|---:|---:|
| astria | 100% | 0% | 0% | 1.92 | 0.879 |
| Graphify | 77.1% | 2.9% | 20% | 1.61 | 0.852 |

This is the first generated-answer-correctness measurement in the project: retrieval precision (100% vs 77% judged correct) tracks the deterministic file/symbol results above. Single judge, single run — not a statistical claim.

**Independent-grader confirmation (OpenRouter, September 28).** The same 35 answer pairs were re-judged by the organizationally external promptfoo/OpenRouter grader (`openai/gpt-4o-mini` via `OPENROUTER_API_KEY`, reset that morning; symmetric 4,000-token budgets; run cost ≈ $0.12, 772k judging tokens — `scripts/bench/quality/out/promptfoo-results-20260928.json`). Verdicts: astria 77.1% pass (27/35), Graphify 65.7% (23/35), 0 errors. The external judge is stricter on both tools than the TypeSafe judge — it fails six questions on *both* sides (community detection, incremental change detection, tree-sitter language coverage, blast-radius, URL-ingestion safety, file-type detection) — but the ordering agrees with every other measurement: astria leads on answer quality under both graders.

Two boundaries on this result. The 35 questions are the self-authored golden set on astria's own repository; they measure answer quality on one corpus, not general correctness. And each grading track is a single judge with a single run — agreement across two independent judges strengthens the ordering but remains a point estimate, not a statistical claim.

## Reserved evaluation expansion (October 5, unmeasured)

The external suite now includes 24 new questions: 12 on Requests 2.32.3 and 12 on Commander 12.1.0, pinned to immutable commits in `scripts/bench/external/corpora.json`. They cover redirects, hooks, cookie merging, adapter selection, streamed bodies, option precedence, lifecycle ordering, help filtering and conflicting options. Questions and expected declaration locations were agent-authored from production source without inspecting retrieval outputs. Source inspection is disclosed; these are not independently human-authored or blind graded questions. They remain **reserved and unexercised** until their first explicit evaluation. No new benchmark was run for this change.

Schema v3 adds a bounded deterministic iterative rg-plus-source baseline alongside the separately named single-pass floor v2 and graph retrieval. Each method receives the same question, corpus and exact delivered token budget. Search refinement uses call/import evidence from read windows, never expected files, symbols or declaration locations. Reports retain per-round terms, source windows, search/read time and bytes/tokens, failures, delivered/raw context, file and exact declaration rankings. Search output and source-read tokens describe internal work, not tokens actually delivered to an LLM. Graph construction is recorded separately per corpus; baseline build cost is zero.

Default runs skip reserved corpora. Explicit opt-in consumes reservation before the first query and writes an exposure record, even if that run subsequently fails. The catalog also protects the existing Click doc-intent reserved split whose filename omits `reserved`. Future results must disclose exposure and separate all three methods and splits. The iterative method is bounded lexical search rather than a skilled agent or compiler; declaration detection is lexical and may miss multiline syntax. One run cannot establish statistical or downstream-task superiority. See the [harness methodology](https://github.com/Nodesify/astria/tree/develop/scripts/bench/quality) for invocation and limits.

## Targeted-search baseline (September 30)

Every comparison above measures astria against Graphify; the question a graph skeptic asks is "what does this buy over just searching?" The historical schema-v2 external runner's deterministic baseline answers the measurable version: question-derived `rg` terms, term-occurrence file ranking, source windows around first matches — single-pass, no iteration, no golden knowledge. Eight symbol-heavy questions over the pinned Click/Express/ripgrep corpora, structural graphs only (CLI 1.0.7 from `b0a9c2e`, dirty tree recorded in provenance), depth 3, identical `o200k_base` complete-line clipping, failed queries kept in denominators. The 250/500 points extend the runner's default 1,000/4,000 sweep; full payloads and per-run provenance are checked in at [`worked/external-baseline/`](https://github.com/Nodesify/astria/tree/develop/worked/external-baseline).

| Budget (tokens) | astria MRR | astria hit@1 | astria hit@5 | rg MRR | rg hit@1 | rg hit@5 | astria avg delivered | rg avg delivered |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 250 | 1.000 | 100% | 100% | 0.000 | 0% | 0% | 207 | 236 |
| 500 | 1.000 | 100% | 100% | 0.000 | 0% | 0% | 474 | 493 |
| 1,000 | 1.000 | 100% | 100% | 0.031 | 0% | 12.5% | 970 | 994 |
| 4,000 | 1.000 | 100% | 100% | 0.092 | 0% | 25% | 3,971 | 3,992 |

astria ranked the defining file first for 8/8 questions at every budget — 184–234 delivered tokens per response at the 250 budget, zero clipping (the engine's internal accounting and the external tokenizer agreed on every response). The baseline never ranked it first: its best ranks at 4,000 tokens were 4, 5, 9, 11 and 12, three questions missed at every budget, and extra context added distractors rather than corrections. Relative to this floor that is ~19× less delivered context for strictly better first-shot ranking (1,652 vs ~31,900 tokens across the set).

Boundaries: the baseline is deterministic single-pass search, not an expert iterating on results — the harness's own caveat, and the honest limit of this comparison; a skilled agent refining queries and reading targeted files would beat this floor, and that comparison is unmeasured. The eight questions name their target symbols — astria's home turf; doc-intent behavior is measured on the paired tracks. One observation per condition, n=8, and graph-build cost (seconds per corpus, once; the baseline has none) is excluded from token accounting.

## Performance tradeoffs

| Corpus | Astria build (s) | Graphify build (s) | Astria mean query (s), 4k | Graphify mean query (s), 4k |
|---|---:|---:|---:|---:|
| Astria self | 10.92 | 13.59 | 1.01 | 2.76 |
| Click | 8.29 | 11.55 | 0.45 | 1.13 |
| Express | 12.29 | 9.55 | 0.53 | 0.82 |
| ripgrep | 14.16 | 14.10 | 0.56 | 1.19 |

Astria remains inside one second per query everywhere and within budget on every response. Chunked documents enlarge the self graph (1,432 cached definitions retained), which lengthens both sides' queries on that corpus; graph builds stay comparable to Graphify's across corpora.

## Conversational-memory retrieval (LoCoMo, full set)

The 1.0.5 assessment recorded near-zero conversational retrieval (full-set recall@10 0.2% structural). The cause was structural: transcript sidecars collapsed into a few content-free nodes, so nothing could match. 1.0.6 chunks document bodies and ranks chunked prose on multi-term coverage; the full 1,977 evidence-backed questions now score, structural only, no LLM and no embeddings:

| recall@1 | recall@3 | recall@5 | recall@10 | MRR |
|---:|---:|---:|---:|---:|
| 63.5% | 79.3% | 84.5% | 84.5% | 0.717 |

Graphify's published LoCoMo recall@10 (~0.497) grades LLM-synthesized answers on a 300-question subset — a different protocol that is not directly comparable — but the order-of-magnitude gap that motivated the transcript work is closed. Ranking failures that remain are mostly same-conversation distractors: the right session ranks, not always the first file.

## LLM enrichment experiment (OpenRouter, deep modes)

Both tools were run in their semantic-enrichment modes on the Click corpus (pinned commit, fresh copies): `astria run . --backend openai --deep` versus `graphify extract . --backend openai --mode deep`, one shared model (`openai/gpt-4o-mini` via the OpenAI-compatible endpoint, `https://openrouter.ai/api/v1`). Astria added 305 deep concept links; Graphify reported 125,962 input / 4,429 output tokens for its semantic pass. Total measured spend across both builds and the query round: **$0.12**.

Retrieval was then scored with the same golden questions, same parser, same budgets as the structural runs:

| Set | astria deep | astria structural | graphify deep | graphify structural |
|---|---|---|---|---|
| Frozen (n=3), MRR / hit@1 / recall@5 | 1.000 / 100% / 100% | 1.000 / 100% / 100% | 0.667 / 33% / 100% | 0.667 / 33% / 100% |
| Additional (n=2), MRR / hit@1 / recall@5 | 0.333 / 0% / 100% | 0.333 / 0% / 100% | 0.500 / 50% / 50% | 0.500 / 50% / 50% |

The deep edges changed nothing measurable on code retrieval: identical scores to the structural graphs for both tools. This is the expected profile of an AST-driven code graph — code questions are answered by exact symbols and paths, not semantic associations — and it argues against paying for enrichment on code corpora. Enrichment cost, answer-quality (blind judging) and doc-heavy corpora are separately measured concerns; the paired runner retags this experiment every run, so the numbers above are reproducible. This is a cost-bounded single run, not a statistical claim.

See the [paired runner](https://github.com/Nodesify/astria/tree/develop/scripts/bench/paired) for corpus pins, unchanged goldens, symbol diagnostics and separately versioned validation cases. Raw measurements were retained locally in `bench-work/paired-verified-20260927`; that ignored directory is not shipped in the repository. The [historical benchmark page](./benchmarks.md) retains its original release context.
