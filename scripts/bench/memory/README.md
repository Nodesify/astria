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

## First measured result

Smoke sample (10 questions, structural graph, no embeddings): recall@10
0.1 — consistent with the self-corpus finding in `../quality/README.md`.
Not a published number; run the full set before quoting anything.

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
