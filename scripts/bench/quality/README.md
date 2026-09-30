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

`.github/workflows/quality.yml` gates proposed changes using the local native build and a 50% self-corpus recall@5 floor. External runs remain explicit opt-in; the September 30 astria-vs-baseline comparison over the Click/Express/ripgrep corpora is checked in at [`worked/external-baseline/`](../../../worked/external-baseline/). Blind promptfoo judging remains optional under `promptfoo/` and is separate from deterministic retrieval metrics.

## Blind answer-correctness judging

`blind-judge.mjs` grades generated answers rather than file rankings. Both tools answer the same golden questions against their own corpus graphs at the same 4,000-token budget; a TypeSafe System One judge (`jev-latest`) scores each answer against the golden rubric (FAIL / PARTIAL / PASS) without knowing which tool produced it, after both answers are truncated to the same character limit.

```sh
TYPESAFE_API_KEY=... node scripts/bench/quality/blind-judge.mjs [--out scripts/bench/quality/out/blind-judge-results.json]
```

Requires `TYPESAFE_API_KEY` (model override via `TYPESAFE_MODEL`, default `jev-latest`). The judge is blind to tool identity but not organizationally independent. The same answer pairs were also graded on September 28 by the external promptfoo/OpenRouter judge (`gpt-4o-mini`, ~$0.12): astria 77.1% pass, Graphify 65.7% - stricter on both tools, same ordering. Verdicts land in `quality/out` and are not published automatically. The September 28 self-corpus run graded astria answers 100% PASS against Graphify 77.1% PASS / 20% FAIL (mean 1.92 vs 1.61 of 2) - single judge, single run, not a statistical claim.


## Judge-layer A/B

`judge-ab.mjs` measures what `--judge jev` changes: it builds (or reuses) one graph per mode over the same corpus — `plain` (structural), `llm` (semantic extraction), `llm-jev` (the judge layered on the backend) — then scores every golden set against every mode at each budget and detail tier with the run-quality file-rank methodology. Graph shape comes from `astria stats --json`; build-output tails and engine/judge configuration land in the results JSON.

```sh
# fresh three-mode build over a corpus (llm/llm-jev bill the engine backend)
node scripts/bench/quality/judge-ab.mjs --corpus /abs/click-checkout

# score prebuilt graphs without any API spend (e.g. the 2026-09-28 mode dirs)
node scripts/bench/quality/judge-ab.mjs \
  --mode-dir plain=bench-work/modes-20260928/plain \
  --mode-dir llm=bench-work/modes-20260928/llm \
  --mode-dir llm-jev=bench-work/modes-20260928/llm-jev
```

`plain` needs no key. `llm` needs an OpenAI-compatible engine key (`--api-key-file`, default the OpenRouter key under `bench-work/llm-exp/or_key.txt`). `llm-jev` additionally needs a Typesafe key (`TYPESAFE_API_KEY`/`ASTRIA_LLM_JUDGE_API_KEY` or `--judge-key-file`); the harness fails before any build spend when a key is missing. The judge gate is a live decision model, so a judged graph is one sample from a distribution (measured gate variance: 31 vs 17 files gated on identical replay in the 2026-09-28 modes run) — rerun before claiming a trend. Results and a markdown report land under `bench-work/judge-ab/<run>/`.
