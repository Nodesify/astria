# Retrieval quality — golden QA + recall@k + blind judging

Token compression says the graph is *cheap*; this layer measures whether it
answers *well*. It runs a golden QA set through the real query engine and
scores the files/symbols each answer surfaces.

## The harness

```bash
# validate the golden set against the current tree (CI-safe, no graph needed)
node scripts/bench/quality/run-quality.mjs --check

# full run against a built graph (astria run . first, if needed)
node scripts/bench/quality/run-quality.mjs --out out/quality-results.json
```

Metrics per question: rank of the best expected file/symbol in the answer
(`hit_rank`), aggregated to `recall@1/3/5/10` and `MRR`. Zero dependencies.

## First measured result (structural graph, no embeddings)

35 questions over this repository's own graph:

| recall@1 | recall@5 | recall@10 | MRR |
|---|---|---|---|
| 5.7% | 8.6% | 17.1% | 0.094 |

That is deliberately unflattering and it is the point: the token benchmark
says answers cost ~3k tokens; this says they rarely surface the *right* file.
The miss mode was consistent and fixable:

1. **Output was hub-ranked, not relevance-ranked** — `query` ordered matched
   nodes by degree (`astria-query`'s render loop), so the load-bearing hubs
   drowned the on-topic node.
2. **Doc nodes dominated seeds** — documentation headings keyword-match
   strongly, and traversal from them rarely reached the implementing crate
   file within the budget.

## After the ranking fixes (same set, same graph, no embeddings)

1. **Answers rank by relevance, not degree** — seed-match score first, then
   traversal distance to the matching seeds, then degree as a tiebreak
   (`astria-query`'s render loop).
2. **Question words are filtered as stopwords** and a node-type prior ranks
   code symbols above prose/stub nodes on equal term evidence
   (`astria-query`'s scorer).

| recall@1 | recall@5 | recall@10 | MRR |
|---|---|---|---|
| 42.9% | 65.7% | 85.7% | 0.537 |

(35/35 answered, avg 0.19 s/query.) Remaining headroom: semantic recall via
`--embed` for paraphrases with no lexical overlap, and per-answer
doc-heading caps; measure before and after with this harness.

## Blind judging (promptfoo)

The deterministic harness scores retrieval; promptfoo judges *answer
quality* blind — both tools answer the same questions, an LLM rubric grades
each answer without knowing which tool produced it.

```bash
# regenerate tests after golden-set changes
node scripts/bench/quality/promptfoo/gen-promptfoo-tests.mjs

# run the matrix (needs a judge key: ANTHROPIC_API_KEY or OPENAI_API_KEY;
# and GRAPHIFY_CMD for the original-tool provider, or delete that provider)
npx promptfoo@latest eval -c scripts/bench/quality/promptfoo/promptfooconfig.yaml \
  --output scripts/bench/quality/out/promptfoo-results.json
```

`promptfooconfig.yaml` wires `astria-provider.mjs` (astria CLI on the corpus
at `BENCH_CORPUS`) against `graphify-provider.mjs` (original Python tool).
Each test carries the ground-truth rubric generated from the golden set.

## Golden set

`golden/astria-self.jsonl` — one JSON object per line:

```json
{"id": "q01", "question": "…", "expected_files": ["crates/…"], "expected_symbols": ["fn_name"]}
```

Expectations are grounded paths: `--check` fails when an entry matches no
real file, so the set cannot silently rot as code moves. Extend the set for
new corpora by adding a jsonl and passing `--golden`; `path/to/file.rs`
entries match as case-insensitive substrings of any surfaced path.
