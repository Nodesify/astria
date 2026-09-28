# Benchmarks

- `paired/`: explicit local-build versus pinned Graphify runner, fresh paired corpora, frozen/corrected/additional-validation tracks, scoped symbol retention and declaration retrieval. See `paired/README.md`.
- `quality/`: exact file hit@k, true recall@k and MRR, including failed queries in the denominator. See its README for schema v2 and limitations.
- `external/`: immutable Click, Express and ripgrep corpora, grounded seed questions, and a question-derived rg plus source-read baseline under 1000/4000 requested token budgets. Run explicitly with `node scripts/bench/external/run.mjs` after building the local CLI and installing js-tiktoken.
- `run-snapshot.mjs`, `orig_run.py`, `tokenize.mjs`: published-release Graphify comparison; corpus/query size ratios are diagnostics, not savings versus targeted search.
- `memory/`: LoCoMo memory retrieval, separate from repository navigation.
- `open/`: external published benchmarks — SWE-bench Verified localization, RepoQA find, HotpotQA distractor retrieval, LOCOMO QA accuracy (Graphify-protocol shape). Smoke subsets by default; see its README for protocols and licenses.

`quality.yml` builds the proposed native source and gates self-corpus retrieval; `bench-snapshot.yml` records installed release measurements. Reports distinguish source/harness commit, CLI version, corpus revision, budgets and tokenizer. No new measurements are supplied by these harness changes. Historical scores require their original methodology; do not relabel them as schema v2 results.
