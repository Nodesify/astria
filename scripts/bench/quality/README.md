# Retrieval quality

Build the native module and CLI from the checkout, install `js-tiktoken` at the repository root, then build a fresh corpus graph. Pass the absolute `packages/astria-cli/dist/index.js` path with `--astria`; JavaScript entrypoints run with Node without shell interpolation. `--astria` also accepts a native executable path, not a shell command prefix.

```sh
node scripts/bench/quality/run-quality.mjs --check
node scripts/bench/quality/run-quality.mjs --astria /absolute/checkout/packages/astria-cli/dist/index.js --budget 4000 --min-recall5 50
```

Schema v2 scores exact normalized, case-sensitive corpus-relative file paths. `hit@k` means at least one expected file appears in the first k unique files. `recall@k` is the fraction of all expected files retrieved, averaged across questions. MRR uses the first expected file. Symbol matches are diagnostics only. Failed, timed-out, nonzero-exit and empty responses remain in every quality denominator as zero. Source paths from NODE records rank before EDGE-only paths.

Reports include CLI version, checkout commit and dirty state, native and entrypoint SHA-256 hashes for local builds, corpus commit and file count, golden SHA-256, runtime, query budget and depth. Local runs require `dist/astria.node` and reject a package-root `astria.node`, which otherwise takes loader precedence. Missing or unloadable native builds fail instead of using the installed platform package. The source commit identifies the harness checkout; a local built artifact must come from that checkout. CI builds it in the same job. Installed binaries may have different source provenance.

Historical v1 scores called any-file-or-symbol hits “recall” and omitted failures. They cannot be compared directly to schema v2. The existing snapshot has not been remeasured by this change.

## External repositories and lexical baseline

`../external/corpora.json` pins Click 8.1.8, Express 4.21.2 and ripgrep 14.1.1 to immutable commits. Eight seed questions include upstream evidence links and source anchors, validated before each run. This small, symbol-heavy set checks basic navigation; it does not establish broad architectural reasoning quality. Expand independently authored, multi-file questions before making general quality claims.

```sh
npm install --no-save js-tiktoken
node scripts/bench/external/run.mjs
```

Requires git, rg, Node 22, and a locally built CLI/native module. The runner clones source only, never installs or executes upstream packages. It refuses dirty (including untracked source), mismatched or previously graphed corpus directories; generated `.astria` files are the only explicit status exclusion. Use a fresh `bench-work/external` directory for another run. Outputs land in `quality/out` and are not published automatically.

Each corpus runs the same questions with requested budgets of 1000 and 4000 tokens for astria and targeted rg plus source reads. The baseline derives search terms solely from the question, collects at most three matching lines per file, ranks by term occurrences (path order breaks ties), and reads a window of 10 lines before and 30 after the first match. Both methods pass through the same o200k_base clipping step before path parsing and quality scoring. Clipping drops any incomplete final line. Reports include delivered tokens, raw tokens and whether clipping occurred. Graph generation also receives the requested CLI budget, but its internal estimate does not govern the final scored context. Golden files and anchors never guide the baseline. The paired suite requires the shared tokenizer and fails if it is unavailable.

Token cost counts delivered retrieval context, not filesystem bytes scanned, graph construction, model reasoning, or a complete agent task. The baseline is deterministic, not a claim about expert iterative search. Full-corpus/query ratios are a separate size diagnostic and do not measure savings against targeted search.

`.github/workflows/quality.yml` gates proposed changes using the local native build and a 50% self-corpus recall@5 floor. External runs remain explicit opt-in; no external measurements are checked in yet. Blind promptfoo judging remains optional under `promptfoo/` and is separate from deterministic retrieval metrics.
