# Memory benchmarks — LoCoMo (and LongMemEval)

LoCoMo (snap-research, ACL 2024) scores long-conversation memory: 10
multi-session conversations, ~2,000 QA pairs, evidence-referenced. astria
ingests conversation transcripts as sidecars, so the benchmark becomes a
retrieval task: does a graph query surface the session where the fact lives?

## Usage

```bash
# 1. dataset -> transcript sidecars + QA file (downloads ~2.8 MB once, cached)
node scripts/bench/memory/prepare-locomo.mjs

# 2. build the graph over the transcript corpus, answer, score
node scripts/bench/memory/run-locomo.mjs                     # full 1,977 evidence-backed QA
node scripts/bench/memory/run-locomo.mjs --limit 50          # smoke run
node scripts/bench/memory/run-locomo.mjs --judge             # + LLM-graded correctness (ANTHROPIC_API_KEY)
node scripts/bench/memory/run-locomo.mjs --no-build          # graph already built
```

Results land in `bench-work/locomo-results.json`: per-question evidence-file
rank plus `recall@1/3/5/10` and `MRR`; with `--judge`, also
`judged_correct` (Claude Haiku comparing the system answer to the reference).

## Measured results (full set, structural, no embeddings)

The full 1,977-question set on the 1.0.6 chunked-document graph: **recall@1
66.1%, recall@3 80.7%, recall@5/10 85.0%, MRR 0.736** (artifact
`bench-work/locomo-fix3-full.json`; the shipped 1.0.6 scored 63.5/79.3/84.5%
with MRR 0.717). Remaining misses are mostly same-conversation distractors —
the right session ranks, not always first. One observation per condition;
treat as a point estimate, not a statistical claim.

History: the pre-1.0.6 structural pipeline scored ~0.1–0.2% recall@10 on this
set (a 10-question smoke first showed 0.1) — transcript sidecars collapsed
into a few content-free nodes, so nothing could match. Chunked document
retrieval closed that gap; don't quote the smoke number as current.

## Why this matters

Upstream Graphify publishes LOCOMO/LongMemEval numbers. This adapter runs
astria through the same evidence-referenced protocol on the same public
dataset — apples-to-apples, with an adapter that works over local files and
needs no API keys for the recall metric.

## License caveat

LoCoMo data is **CC BY-NC 4.0** — research/non-commercial use. Do not wire
it into a pipeline that ships dataset content. LongMemEval (ICLR 2025,
xiaochengYang/LongMemEval) is the same adapter pattern; its dataset comes
from Hugging Face and is larger — porting is mechanical (prepare → QA jsonl
with evidence ids → run) and left as follow-up until needed.
