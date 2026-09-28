# Open benchmarks

Adapters for external, published benchmarks — the evaluations other systems
already report numbers on — alongside the self-authored suites in `paired/`,
`quality/`, and `memory/`. The goal is comparability: Graphify's published
table (`bench-work/corpus/BENCHMARKS.md` in the pinned checkout) reports
LOCOMO + LongMemEval QA accuracy with a key-fact-coverage judge and an
ERPNext code-QA suite; these adapters put astria on the same public datasets
plus the code-localization standards.

Every run writes a `provenance` block (dataset, license, model, protocol
notes) into its results JSON. Smoke subsets are labeled as such — they are
pipeline validation, not benchmark claims. Full runs use the same commands
with larger n.

| suite | dataset (license) | metric | adapter |
|---|---|---|---|
| SWE-bench Verified localization | princeton-nlp/SWE-bench_Verified (MIT) | file-level hit@1/3/5, MRR vs developer-patch files | `swebench.mjs` |
| RepoQA find (retrieval variant) | evalplus/repoqa dev-dataset (Apache-2.0) | file hit@1/5/10, function-rank MRR vs needle description | `repoqa.mjs` |
| HotpotQA distractor retrieval | hotpotqa/hotpot_qa validation (CC BY-SA 4.0) | supporting-document hit@1/5, MRR, both-gold-in-5 | `hotpotqa.mjs` |
| LOCOMO QA accuracy (Graphify-protocol shape) | snap-research LoCoMo (via `memory/prepare-locomo.mjs`) | key-fact coverage, accuracy at ≥0.5 coverage | `locomo-qa.mjs` |

## Audit outcomes (2026-09-28) — bugs found and fixed

The first RepoQA smoke silently graded 1 repo instead of 4 and 10 needles of 600. The audit behind that number found and fixed:

- **Duplicate node ids in document extraction** (build-breaking): repeated RST section titles ("API Changes" in changelogs) and punctuation-only headings ("# ...", empty slug collapsing to the file node's id) produced duplicate ids within one extraction. Both now ordinal-suffix or fall back deterministically; regression-tested in `astria-extract`.
- **Silent sample shrinkage** (harness): `repoqa.mjs` logged build failures and `continue`d, reporting "first 4 repos, 10 needles" as if intended. It now records `failed_repos` in the results and exits nonzero — an incomplete sample must never look like a benchmark number.
- **Query-term dilution** (ranking): RepoQA-style multi-sentence descriptions let nodes matching many weak terms outrank the node matching the query's rare, identifying terms. Query scoring now scales nodes by coverage of the query's highest-IDF (salient) terms; this recovered file/function ranks across the 4-repo sample and lifted HotpotQA both-gold-in-5 from 0.46 to 0.50 with no code-nav golden regressions.

Known gaps, measured but not yet fixed:

- **Embedding seed reservation landed; the binding constraint is now model quality.** Queries reserve one traversal seed for the best semantic-only candidate (embedding evidence above a floor, zero token evidence — `ASTRIA_QUERY_DEBUG_SCORES=1` shows the reserved slot), and the seed slot is covered by unit tests. Measured on psf/black, the reserved candidate is the needle function in 0 of 10 cases: the local embedding model does not rank an abstract natural-language description's true function top among ~2.8k nodes. Closing the RepoQA embed gap further needs a better query↔code embedding (or LLM semantic seeds), not graph-wiring changes.
- **RepoQA function-level retrieval remains weak** (func hit@1 0.25 over 40 needles): remaining misses are file-node label matches on common words out-seeding docstring evidence — needs a scoring debug surface (per-node score dumps) rather than more blind knob turns.

Smoke numbers below are pipeline validation only (n=100 HotpotQA, n=40 RepoQA needles), but the samples are now complete and reproducible.

## SWE-bench Verified localization

The localization evaluation the agent literature reports (LocAgent, Agentless
and successors): issue text in, ranked files out, graded against the files
the developer patch edits. Per instance: `git checkout base_commit`, build
the structural graph (no LLM), `query <problem_statement> --budget 4000
--depth 2`, rank files by first NODE appearance. Primary metric grades the
non-test developer-patch files; the all-patch variant is recorded too.

```bash
python scripts/bench/open/prepare-swebench.py           # 500 rows -> swebench_verified.jsonl
git clone --filter=blob:none --no-checkout https://github.com/django/django.git bench-work/open/swe-repos/django
node scripts/bench/open/swebench.mjs 25 django/django   # smoke: first 25 django instances
node scripts/bench/open/swebench.mjs 231 django/django  # full django slice
```

Each instance is built once and queried under four configurations — raw vs
cleaned issue text (traceback/file-line noise stripped), budget 4k/8k, depth
2/3 — so deltas are within-instance comparable. Function-level grading
parses gold function names from patch hunk headers (`func_hit1/5/func_mrr`
per configuration).

## RepoQA find

RepoQA pins 60 repos (10 per language) at fixed commits; each repo has 10
"needles" — a function and a natural-language description of it. The
official task prompts an LLM with the repo and grades Hit@1 on the function
name; this adapter measures the retrieval substrate (graph query from the
description, no LLM), grading the needle's file (hit@1/5/10) and the
function's rank in NODE labels. Function identity is exactly the official
target; file hits are the coarser view comparable to other retrieval stacks.

```bash
node scripts/bench/open/repoqa.mjs python 4   # first 4 python repos, 10 needles each
EMBED=1 node scripts/bench/open/repoqa.mjs python 10   # embedding arm (local model, --embed builds)
```

`EMBED=1` builds each repo with `--embed` (local model) and writes
`repoqa-results-<lang>-embed.json` for a paired comparison against the
baseline arm on the same repo set.

## HotpotQA distractor retrieval

Each distractor question ships 10 Wikipedia paragraphs, exactly 2 of which
are the gold supporting documents. Paragraphs become namespaced markdown
files (`qNNN--Title.md`), one graph is built over the whole corpus, and each
question is a graph query; grade supporting-document retrieval. The
multi-hop QA leg (reader + judge over retrieved context) is the same shape
as `locomo-qa.mjs` and not yet wired for this dataset.

```bash
node scripts/bench/open/hotpotqa.mjs 100      # first 100 validation questions
node scripts/bench/open/hotpotqa.mjs 7405     # full validation split
```

Three variants are graded per question: `single` (the question alone),
`twostage` (re-query seeded with stage-1's top NODE labels — measured worse,
recorded as a negative result), and `merged` (stage-1 ranking with stage-2
discoveries appended — preserves single's quality and adds second-hop gold
documents, lifting both-gold-in-5).

## LOCOMO QA accuracy (Graphify-protocol shape)

Follows the shape of Graphify's published memory table: reader answers from
retrieved context, judge grades key-fact coverage with
`(covered + 0.5*partial) / total`, accuracy = share of answers with
coverage ≥ 0.5. Disclosed differences: reader/judge are gpt-4o-mini
(Graphify used one shared Kimi K2.6 for every LLM role), key facts come from
the LoCoMo reference answer rather than a precomputed atomic set, and a
single judge without second-judge validation. Directional, not
cross-publishable, until both systems run under one shared model.

```bash
node scripts/bench/open/locomo-qa.mjs 30      # smoke
node scripts/bench/open/locomo-qa.mjs 1727    # all non-adversarial QA
```

## Measurement limits

One observation per condition; smoke n is small; judge-based legs inherit
single-judge variance. Datasets are downloaded from their canonical hosts at
run time and are not redistributed by this repository.
