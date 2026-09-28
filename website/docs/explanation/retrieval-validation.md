---
title: Retrieval validation (September 2026)
description: Paired local retrieval measurements, symbol preservation and performance tradeoffs.
sidebar_position: 3
---

# Retrieval validation (September 2026)

Latest run: September 27 evening, Windows, Ryzen AI 9 HX 370, Node 24, Python 3.12.12. The candidate is the 1.0.6 tree at `cbe8364` (chunked document retrieval, cross-conversation ranking, chunk overlap, introspection commands); Graphify v0.9.69 is pinned to `4139885a1212956cf69a76946fbde0d181ab85e9`. Each tool receives fresh archives of the same pinned corpora. Structural extraction only, no LLMs or embeddings, BFS depth 2, exact shared `o200k_base` counting and identical complete-line clipping for scoring. Raw budget overruns are counted before clipping. Each condition has one observation including process startup; timing differences are not statistically established.

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

All query processes succeeded across the eight corpus/split builds. Astria stayed within budget for every response at both budgets (Graphify exceeded its own 1,000-token budget on 29/35 self questions). Every extraction-cache definition ID survived publication (1,432/1,432 self, 1,147/1,147 Click, 3,263/3,263 Express, 3,076/3,076 ripgrep). All known Click/Express/ripgrep-heldout definitions were present at their implementation lines (5/5, 2/2+1/1, 4/4).

Exact-symbol top-five recall is weaker than file recall, and the September 28 ranking fixes narrowed the gap on the 1.0.6 pinned graphs: the additional Click cases moved from 0% to 2/2 definitions surfaced (the missing `BaseCommand.get_usage` had lost a seed slot to a same-name label tie and now reserves one via its qualified scope), the additional ripgrep cases from 25% to 3/4 (the remaining miss ranks 7th), and the self code set's MRR rose from 0.618 (paired run) to 0.687 at unchanged recall. Two same-name `invoke` implementations on Click still land at ranks 6 and 9 — genuinely ambiguous unqualified prose over four same-name symbols.

The validation surface also grew: `scripts/bench/paired/*.heldout-v2.jsonl` adds 14 separately authored cases (5 Click, 5 Express, 4 ripgrep) grounded in the pinned corpora with line-exact definitions. On the current runtime they retrieve at 5/5, 5/5, and 2/4 files respectively (the two ripgrep misses are lexical hijacks by `command`-helper chunks), with 11/13 v2 definitions inside the top five. These cases were exercised during verification and are not untouched held-out evidence.

## Blind answer-correctness judging (TypeSafe, September 28)

The blind judging blocked since the 1.0.4 comparison — every OpenRouter judge call failed on an exhausted key — ran with a TypeSafe System One judge (`jev-latest`, score-style graded verdicts) via `scripts/bench/quality/blind-judge.mjs`. Both tools answered the same 35 rubric-grounded questions on the self corpus, and the judge graded each answer against the golden rubric without knowing which tool produced it:

| Tool | PASS | PARTIAL | FAIL | mean score (0–2) | mean judge confidence |
|---|---:|---:|---:|---:|---:|
| astria | 100% | 0% | 0% | 1.92 | 0.879 |
| Graphify | 77.1% | 2.9% | 20% | 1.61 | 0.852 |

This is the first generated-answer-correctness measurement in the project: retrieval precision (100% vs 77% judged correct) tracks the deterministic file/symbol results above. Single judge, single run — not a statistical claim.

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
