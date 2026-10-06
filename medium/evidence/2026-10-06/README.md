# October 6, 2026 paired evaluation evidence

This package supports the Medium article “How We Evaluate Code Retrieval Tools.” It exports existing measurements; no new benchmark was run to prepare it. Astria's maintainers performed the evaluation. The question sets were previously exercised, and the measurements do not establish downstream agent-task success or general superiority.

## Included artifacts

- [Original report](report.md): copied from the saved run, preserving all eight corpus/split conditions and definition misses.
- [JSON summary](summary.json): method, source commits, measured binary hashes, question hashes, build counts/times, and retrieval summaries. It omits raw responses and the detailed symbol-retention audit.
- [CSV summary](summary.csv): 32 rows, one per corpus/split × tool × budget. `avg_tokens` measures delivered output after shared clipping; `avg_raw_tokens` and `over_budget` describe the original response. Exact-definition diagnostics remain in the report and raw results.
- [Original results, gzip-compressed](results.json.gz): the complete historical `results.json`, including query responses, metrics, timings, provenance, and symbol-retention diagnostics. After decompression, its SHA-256 must match `original_results_sha256` in `summary.json`. It retains local execution paths as recorded by the runner; they are provenance, not paths a reader should reuse.
- [Configuration template](config.template.json): the original eight-condition configuration with machine-specific checkout/Python paths replaced by explicit placeholders and question inputs redirected to this package. Budgets are explicitly 1,000 and 4,000 tokens. This is a portable template, not a byte-identical copy of the historical configuration.
- `inputs/`: byte-for-byte copies of the measured question files and declaration sidecars. No reserved evaluation sets are included.

These files are ready for publication but are currently local. Upload them together to a public repository or release attachment, preserving this directory layout, then replace the article's relative evidence links with the actual public URLs. Do not describe this package as public until it is accessible.

## Pins and environment

| Input | Full commit |
| --- | --- |
| Astria source, runner, and self corpus | `5e69c63cee176a7addc3b5c4414280bba3d2d815` |
| Graphify 0.9.77 | `5c7b84792f453582676548185aaec3824d51dfe2` |
| Click | `934813e4d421071a1b3db3973c02fe2721359a6e` |
| Express | `1faf228935aa0a13111f92c28ee795be64ce3f0f` |
| ripgrep | `4649aa9700619f94cf9c66876e9549d83420e16c` |

Historical environment: Windows, AMD Ryzen AI 9 HX 370, Node v24.15.0, Python 3.12.12, release-profile Astria 1.1.0. The recorded Astria and Graphify source-status fields were empty. Binary hashes establish the identity of measured artifacts; a source commit alone does not prove how a binary was built. Binaries and full dependency environments are not bundled here, so the package supports rerunning the protocol rather than guaranteeing a bit-for-bit environment or identical scores.

## Prepare and run

Use Astria source at the pin above, with this evidence package available at `medium/evidence/2026-10-06/`. Follow the [pinned contribution/build instructions](https://github.com/Nodesify/astria/blob/5e69c63cee176a7addc3b5c4414280bba3d2d815/CONTRIBUTING.md) to build the native library and CLI, and install the runner's `js-tiktoken` dependency. Retain your build logs. A globally installed CLI is insufficient: the runner uses the supplied CLI distribution and native binary together.

Prepare a clean Graphify checkout at its full pin. Install it editable into a dedicated Python environment using that environment's Python:

```sh
python -m pip install -e PATH_TO_PINNED_GRAPHIFY_CHECKOUT
```

Replace the placeholder with the checkout path, quoting paths containing spaces. Use Git clones of the external corpora that contain their pinned commits. The runner makes fresh archives itself; existing extraction graphs are not reused.

Copy `config.template.json` to `config.local.json`, then set:

- `python`: the dedicated environment's Python executable; for example, a Windows virtual environment's `Scripts/python.exe`.
- `graphify_source`: the clean, pinned Graphify checkout.
- `astria_cli`: your built `dist/index.js`, with its package's CLI dependencies installed in the sibling `node_modules` directory.
- `astria_native`: your release native library. The template's `target/release/astria_napi.dll` is Windows-specific; choose your platform's actual artifact.
- Each corpus `source`: a Git clone containing its pinned commit. The self corpus uses `.`.
- `output`: a directory that does not already exist. Keep the eight corpora, full commit pins, input files, and budgets unchanged when reproducing this protocol.

All paths are relative to the repository root where the command runs, not to the configuration's directory. With the template's output path:

```sh
node scripts/bench/paired/run.mjs medium/evidence/2026-10-06/config.local.json
node scripts/bench/paired/report.mjs bench-work/medium-reproduction-20261006/results.json
```

These are the runner's documented invocations with the supplied template paths. They were not executed during this editorial revision. The runner checks Graphify's imported module and commit, records native/CLI hashes and source status, archives fresh corpus sources, and writes complete raw results. See the [pinned runner README](https://github.com/Nodesify/astria/blob/5e69c63cee176a7addc3b5c4414280bba3d2d815/scripts/bench/paired/README.md) for details.

Compare each corpus/split separately. Eight conditions do not mean eight independent repositories; the 85 responses at each budget include overlapping self splits. Repetitions, independent new questions, and complete agent-task evaluations are separate future work. Do not relabel a rerun of these inputs as untouched held-out evidence.

## Publishing the correctness-story evidence

The separate phantom-dependency article cites the [fix commit](https://github.com/Nodesify/astria/commit/97a83dc605c154bea9362722107045fa843a9e74), the [pinned release write-up](https://github.com/Nodesify/astria/blob/c7789947c8d876aef6a66537ff0aa248560f3c08/website/blog/2026-10-06-astria-1-1-0.md), and the [pinned changelog](https://github.com/Nodesify/astria/blob/c7789947c8d876aef6a66537ff0aa248560f3c08/CHANGELOG.md#L15-L20). Those are maintainer-recorded before/after observations. This paired-run package does not contain the separate raw pre-fix/post-fix graphs or quality outputs, and cannot independently substantiate that correction experiment. If those artifacts are published later, add their links beside the bug article's numerical claim.
