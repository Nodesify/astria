# Retrieval metric contract

`run-quality.mjs` requires measured file recall@5 and grounded declaration
recall@5 of at least 50%, zero query/lexical-search failures, and exact delivered
token-budget compliance. Astria additionally must fit its raw response in the
budget; harness clipping cannot hide an engine budget violation. The floors are
policy choices, not claims about measured performance. Declaration expectations
use verified source paths and one-based declaration lines, independently of
symbol-label diagnostics. Both recalls include failed queries in their denominator.

`--compare measured-reference.json` blocks decreases in file recall@5/MRR and
declaration recall@5/MRR. It rejects mismatched corpus content, golden content,
harness content, method, tokenizer, depth, top-k, budget, extraction/line contract,
graph build configuration, or context policy.
Different engine artifacts are expected; their hashes and commits are provenance,
not comparison identity. Failed or definition-free references cannot support a
comparison. Raw budget overshoot in an older engine is recorded but does not
prevent a newer engine from improving it while retaining retrieval quality.

CI builds the PR base and candidate separately and evaluates both with the current
harness on identical base-commit corpus copies. This measures engine regressions;
it does not measure corpus edits in the PR. Baseline generation uses `--record-only`
to retain historical deficiencies; the candidate still must meet every gate.
Workflow dispatch compares the selected revision with itself plus absolute gates.
Golden declarations must be grounded in the frozen corpus; a changed declaration
requires explicitly revising the evaluation corpus/golden, not relaxing identity.

For an independently reviewed persistent reference, first measure and inspect a
non-reserved run, then explicitly publish it to a **new** file:

```sh
node scripts/bench/quality/establish-baseline.mjs /eval/measured-results.json /eval/reference-v1.json
node scripts/bench/quality/run-quality.mjs --compare /eval/reference-v1.json --out /eval/candidate-results.json
```

Reference publication requires passing gates and never overwrites a file. Routine
evaluation cannot update a reference. Reserved suites require their existing
explicit opt-in and are excluded from CI and reference publication. No new
measurements were run while implementing this contract.

Keep reference artifacts outside the evaluated corpus; adding a reference to the
source tree changes its content identity. The default `scripts/bench/quality/out`
directory is excluded from corpus hashing. The self suite currently grounds three
declaration questions; `definition_questions` exposes that limited coverage.
