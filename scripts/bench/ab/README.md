# Version-to-version A/B benchmark

Measures the cost of a change between two astria builds on the same machine:
build both versions' native modules, run the identical workload against each,
interleave the runs, compare medians.

## What it measures

One driver process per measurement, against one native module and one corpus:

| Phase | What it exercises |
|---|---|
| `cold_pipeline` | fresh `run`: extraction, build, cluster, analysis, report, artifacts |
| `update_noop` | `update` with nothing changed (rebuild-skip path) |
| `stats` | one graph load (`graphStats`) |
| `query_1` … `query_5` | five queries in one process — the 1st pays graph load, the rest show snapshot-cache behavior |
| `repo_map`, `god_nodes`, `explain_node` | graph-load-dominated read tools |
| `export_json`, `export_html` | artifact export |

## Usage

```bash
# 1. Build both versions (current checkout and the baseline, e.g. in a worktree)
cargo build --release -p astria-napi                      # in the current checkout
git worktree add ../astria-prev <baseline-commit>         # baseline
(cd ../astria-prev && cargo build --release -p astria-napi)

# 2. Stage both modules where the driver can load them (the .node
#    extension is required — Node treats a bare .dll as JavaScript)
mkdir -p modules/new modules/prev
cp target/release/astria_napi.dll modules/new/astria.node
cp ../astria-prev/target/release/astria_napi.dll modules/prev/astria.node

# 3. Prepare a corpus: a source tree copy without .git / target /
#    node_modules / .astria / prebuilt binaries
robocopy <repo> corpus /E /XD .git target node_modules .astria bench-work

# 4. Warm up once per side, then run interleaved rounds
node scripts/bench/ab/driver.mjs "$PWD/modules/new/astria.node"  "$PWD/corpus" new 0
node scripts/bench/ab/driver.mjs "$PWD/modules/prev/astria.node" "$PWD/corpus" prev 0
for r in 1 2 3; do
  node scripts/bench/ab/driver.mjs "$PWD/modules/new/astria.node"  "$PWD/corpus" new $r
  node scripts/bench/ab/driver.mjs "$PWD/modules/prev/astria.node" "$PWD/corpus" prev $r
done | tee rounds.jsonl
```

Each run prints one JSON line; take medians per phase per label. Notes:

- The driver deletes every `ASTRIA_*` env var before loading the module, so
  both sides run the deterministic no-LLM configuration.
- Every driver invocation is a fresh process; `cold_pipeline` removes
  `.astria` first, so cold runs never inherit caches.
- Interleaving new/prev per round keeps filesystem-warmth drift symmetric.
- HTML export uses `--mode large` (the corpus exceeds the 5,000-node
  standard-mode limit).

## October 2026 result (d43bc92 → e12af80)

Corpus: this repository's source (1,193 files, ~30 MB), Windows, release
builds, medians of 3 interleaved rounds after a warmup. Full table and
reading: see the "Version-to-version A/B" section of
`website/docs/explanation/benchmarks.md`. Headline: build paths at parity,
read paths 1.2–7× faster (generation-keyed snapshot cache); the harness
caught a no-op-update regression (unextractable binaries flagged as pending
media) that shipped fixed in `e12af80`.
