# Retrieval quality

Build the native module and CLI from the checkout, install `js-tiktoken` at the repository root, then build a fresh corpus graph. Pass the absolute `packages/astria-cli/dist/index.js` path with `--astria`; JavaScript entrypoints run with Node without shell interpolation. `--astria` also accepts a native executable path, not a shell command prefix.

```sh
node scripts/bench/quality/run-quality.mjs --check
node scripts/bench/quality/run-quality.mjs --astria /absolute/checkout/packages/astria-cli/dist/index.js --budget 4000
```

The run gates on file and declaration recall@5 (default policy floors 50%), zero
query/search failures and exact token-budget compliance. `--compare` additionally
requires comparable corpus/method identity and no recall or MRR regression against
a measured reference. See [the metric contract and reference workflow](GATE.md).

Schema v4 scores exact normalized, case-sensitive corpus-relative file paths. `hit@k` means at least one expected file appears in the first k unique files. `recall@k` is the fraction of all expected files retrieved, averaged across questions. MRR uses the first expected file. Symbol-label matches remain diagnostics. Grounded `definitions` (path, one-based declaration line, source anchor) add declaration recall@k and MRR separately. Failed cases retain null declaration ranks. Failed, timed-out, nonzero-exit and empty responses remain in every quality denominator as zero. Source paths from NODE records rank before EDGE-only paths. Older schemas require remeasurement; no line-number conversion is guessed from historical artifacts.

Reports include CLI version, checkout commit and dirty state, native and entrypoint SHA-256 hashes for local builds, corpus commit and file count, golden SHA-256, runtime, query budget and depth. Local runs require `dist/astria.node` and reject a package-root `astria.node`, which otherwise takes loader precedence. Missing or unloadable native builds fail instead of using the installed platform package. The source commit identifies the harness checkout; a local built artifact must come from that checkout. CI builds it in the same job. Installed binaries may have different source provenance.

Historical v1 scores called any-file-or-symbol hits “recall” and omitted failures. They cannot be compared directly to schema v2. The existing snapshot has not been remeasured by this change.

## External repositories and lexical baselines

The catalog in `../external/corpora.json` pins five repositories. The original Click/Express/ripgrep cases have already been exercised. Requests 2.32.3 and Commander 12.1.0 add **24 reserved, unexercised questions** (12 each), with one-based declaration locations, production-source evidence anchors and immutable upstream links. These cases were agent-authored from pinned source without viewing retrieval outputs. Source inspection is disclosed; they are not independent human ground truth or blind judging. No new retrieval measurements accompanied this implementation.

```sh
npm install --no-save js-tiktoken
# Runs only previously exercised corpora by default. Always choose a new directory.
node scripts/bench/external/run.mjs --output bench-work/external-next
# Future first evaluation only: consumes reservation before the first query.
node scripts/bench/external/run.mjs --output bench-work/external-reserved-first --include-reserved
```

Requires git, rg, Node 22 and locally built CLI/native artifacts. Source clones are never installed or executed. The runner verifies clean immutable checkouts, exact source anchors and the recorded SHA-256 of pinned Git blob bytes (read without decoding or checkout line-ending conversion), explicitly disables LLM enrichment (`ASTRIA_LLM_BACKEND=none`), builds without embeddings, and refuses an existing output directory. The output contains fresh corpus graphs, per-method query results and `suite.json` with CLI/native hashes, source status and **separate per-corpus graph build elapsed time**. This practical build time includes CLI post-build work; it is not pure extraction time. Baselines need no graph build. Query timing includes process startup; one observation per condition does not establish significance.

At each 1,000/4,000-token budget and depth 3, compare three separately named conditions:

| Method | Search policy | Limits |
|---|---|---|
| `astria` | Graph retrieval | Requested budget and shared exact complete-line clipping |
| `question-rg-single-pass-floor-v2` | Question terms, fixed-string rg, source windows | One round, up to 24 files |
| `question-rg-iterative-source-v1` | Question terms followed by call/import evidence from read windows; existing relative imports can add candidate files | Up to 3 rounds, 6 unread files per round |

Both lexical methods accept only question, source root and limits: no expected files, definitions or anchors enter search or refinement. Terms are capped at 20 initially and 12 in refinements, with at most 2,000 considered matches per round, three rg matches per file, a 30-second search timeout, 16 MiB search-output buffer and 256 KiB read cap per file. File ranking counts matched terms; path order breaks ties. Windows contain ten lines before and thirty after the first match. Lexical declaration recognition emits path/line records and can miss multiline or language-specific syntax. Refinement and file ranking are deterministic, bounded lexical heuristics; they do not simulate an expert agent or guarantee complete import resolution.

Search output is sorted by path before applying match caps; ties use codepoint path order. Search and import reads exclude graph/build artifacts, benchmark goldens/results and JSONL files to keep labels outside retrieval. This deliberately narrows lexical eligibility on the self corpus; use the fresh external source corpora for fair source-only comparisons, and disclose eligibility differences on document/data-heavy corpora.

Each query retains per-round terms, match counts, read files/windows, refinements, search/read elapsed time, search-output bytes/tokens, source-read bytes/tokens and failures. These costs count actual bounded reads and captured search output; they do **not** count filesystem bytes scanned by rg or model reasoning. Partial search/read failures remain visible even if the baseline delivers some context, and summaries count these separately. Delivered/raw tokens, clipping, distinct file ranking and exact declaration ranking are scored after the same `o200k_base` complete-line clipping for all three methods. Read cost and search-output tokens are internal work, not delivered LLM context. Expected declaration paths/lines are used only in grounding and scoring.

Historical September 30 outputs use schema v2 and `question-rg-plus-source-windows`: one pass with different regex and formatting behavior. The new floor v2 changes fixed-string search, bounds, line labels and declaration records; do not compare it to the old floor under the same method identity. Those checked-in results remain historical and have not been remeasured. Full-corpus/query ratios remain size diagnostics, not measured savings over targeted search.

## Reserved-set protection

`../reserved-corpora.json` protects reserved files even when filenames omit “reserved,” including Click's documentation-intent split. Quality runs require `--allow-reserved`; the external suite requires `--include-reserved`; paired configs require `allow_reserved: true`. Before first retrieval each runner persists an exposure record (quality: output plus `.reserved-exposure.json`; external: `suite.json`; paired: `results.json`). An interrupted run also consumes reservation. Keep these records, label subsequent runs exercised and never tune against a reserved set before its first evaluation. Static `evaluation_status: unexercised` metadata records authoring status, not a durable lock across machines; exposure records override it. The guard blocks accidental evaluation, not intentional copying/renaming of datasets.

Direct invocation against an already prepared corpus uses `--baseline` for floor v2 or `--iterative-baseline` for iterative v1. `--check` validates schema/source grounding without querying and does not consume reservation. New cases and methods are implementation-only here: no benchmark or tests were run.

The self-corpus recall gate in `.github/workflows/quality.yml` remains separate from opt-in external evaluation and optional blind answer judging.

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
