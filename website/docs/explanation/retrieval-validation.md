---
title: Retrieval validation (September 2026)
description: Paired local retrieval measurements, symbol preservation and performance tradeoffs.
sidebar_position: 3
---

# Retrieval validation (September 2026)

September 27 workstation run: Windows, Ryzen AI 9 HX 370, Node 24.15, Python 3.12.12. The candidate release binary contains the retrieval changes after `71e70ef`; Graphify v0.9.69 is pinned to `4139885a1212956cf69a76946fbde0d181ab85e9`. Each tool receives fresh archives of the same pinned corpora. Structural extraction only, no LLMs or embeddings, BFS depth 2, exact shared `o200k_base` counting and identical complete-line clipping for scoring. Raw budget overruns are counted before clipping. Each condition has one observation including process startup; timing differences are not statistically established.

## Frozen file retrieval

Original questions, recall among the first five distinct returned files, at 4,000 tokens:

| Corpus | Questions | Previous Astria | Candidate Astria | Graphify |
|---|---:|---:|---:|---:|
| Astria self | 35 | 77.1% | 78.6% | 67.1% |
| Click | 3 | 66.7% | 100% | 100% |
| Express | 2 | 0% | 100% | 100% |
| ripgrep | 3 | 100% | 100% | 100% |

Self MRR decreased from 0.694 to 0.674 and hit@5 from 85.7% to 82.9%; the changes do not improve every ranking. Seven separately authored validation questions achieved recall@5 of 100%/100%/66.7% for Click/Express/ripgrep, versus 50%/50%/33.3% for Graphify. These cases have been exercised during development and are not untouched held-out evidence. The small external sets do not establish general superiority.

## Correctness diagnostics

All 200 query processes succeeded. Astria stayed within budget for all 100 responses across 1,000/4,000-token budgets. CLI/MCP diagnostic text matched exactly. All seven known Click/Express definitions were present at their implementation lines. Updating an old Click graph restored 151 lost scoped IDs.

Exact-symbol top-five recall remains weaker than file recall: 60% on the Click diagnostics, 0% on the additional Click cases, 100% on Express diagnostics, and 25% on the additional ripgrep cases. Returning the right file does not prove the right symbol or answer was retrieved. No generated-answer correctness was measured.

## Performance tradeoffs

| Corpus | Astria build (s) | Graphify build (s) | Astria mean query (s), 4k | Graphify mean query (s), 4k |
|---|---:|---:|---:|---:|
| Astria self | 9.63 | 14.55 | 0.307 | 0.722 |
| Click | 4.83 | 7.14 | 0.369 | 0.880 |
| Express | 10.15 | 6.95 | 0.331 | 1.050 |
| ripgrep | 10.83 | 10.74 | 0.289 | 0.623 |

Previous Astria query means were 0.14–0.16 seconds. Exact token accounting and larger graphs add cost. Express now extracts many more functions and callbacks; its build is slower than Graphify in this run.

See the [paired runner](https://github.com/Nodesify/astria/tree/develop/scripts/bench/paired) for corpus pins, unchanged goldens, symbol diagnostics and separately versioned validation cases. Raw measurements were retained locally in `bench-work/paired-verified-20260927`; that ignored directory is not shipped in the repository. The [historical benchmark page](./benchmarks.md) retains its original release context.
