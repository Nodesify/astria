# Paired downstream task evaluation

This workflow measures completed agent tasks, rather than treating retrieval recall or a smaller retrieved response as task success. The runner inspects supplied copies and starts no agent unless `--run` is supplied. The separate preparation command creates fresh external clones only with explicit `--prepare`; it never changes the shared source checkout, installs dependencies, or starts an agent. It has not been exercised against an agent. Requests and Commander reserved suites are neither read nor executed by this workflow.

The example manifest contains two concrete Astria maintenance tasks at commit `2509bb184a2bd886a8f0492635ea0556af98643d`: failed-run lifetime accounting and watcher startup/spawn completion. These are known diagnostics derived from source inspection, not untouched held-out tasks. Do not generalize their results to arbitrary repositories. Author additional tasks and freeze their prompts/rubrics before observing candidate results.

## Prepare explicit inputs

The bundled Codex adapter works with the installed Codex CLI (`exec --json`,
`--ephemeral`, `--ignore-user-config`, `--output-last-message`). Its help and
version were inspected on CLI 0.140.0; no paid session was started. Authenticate
Codex through its normal login separately. Set the manifest's model to a model
your account can use and `codex_binary` to the direct executable (on Windows use
the installed `codex.exe`, not an npm `.cmd` wrapper). The adapter deliberately
accepts only supported settings: reasoning effort, network disabled, workspace
write sandbox, Codex binary and Astria CLI entrypoint. It does not pretend to
control temperature or output-token limits unsupported by this CLI.

To prepare the example's four independent pinned inputs from an existing Git
repository that contains the example commit, first build the candidate CLI/native
artifact, then invoke the explicit setup command:

```powershell
node scripts/bench/tasks/prepare.mjs scripts/bench/tasks/manifest.example.json C:/Nodesify/nodesify-graphify C:/eval/new-run-001 C:/Nodesify/nodesify-graphify/packages/astria-cli/dist/index.js C:/Nodesify/nodesify-graphify/packages/astria-cli/dist/astria.node --prepare
node scripts/bench/tasks/run.mjs C:/eval/new-run-001/manifest.json
```

The destination must not exist and must be outside the shared/source checkout.
Edit `codex_binary` in a copied template if PATH has no direct executable. Setup
validates all commit pins and source anchors before copying, clones without
hardlinks into new directories, selects the pin only in those new clones, and
runs structural indexing with `--backend none`. It captures actual whole-process
monotonic build time, CLI/native/graph hashes, and an evidence trace attesting
that no updates have yet occurred. It generates a ready-to-validate manifest
with runtime fingerprints and measured indexing artifacts. No setup was run
during implementation. A failure may leave partial disposable inputs; retain
them for inspection and use a new destination for another attempt.

For external repositories, author a manifest with real maintenance prompts,
full commit pins, source grounding, exact allowed paths and frozen rubrics.
Point this same command at a locally obtained repository containing those pins;
the setup command never fetches an arbitrary moving remote branch. Every task
in one preparation uses that source repository. Prepare distinct manifests for
distinct repositories. The bundled tasks make setup concrete; they remain
known self-repository diagnostics rather than external generalization evidence.

Copy `manifest.example.json` outside this checkout and set its paths, output, agent executable, model and settings. All paths in a manifest resolve relative to that manifest. Supply **four existing independent Git copies**, one baseline/Astria pair per task. Each must be a repository root with a clean pinned HEAD. No directory may overlap this main checkout, another task input, or another condition. The runner inspects commits and source grounding and refuses dirty inputs. It never clones, resets, switches branches, or uses worktrees. Prepare disposable copies yourself; an agent may edit its supplied copy when explicitly run.

Baseline copies must contain neither `.astria` nor `.graphify`. Build the two Astria copies with the exact same candidate CLI/native artifact. Capture actual elapsed build time, binary SHA-256, graph SHA-256, and commit in a separate indexing JSON artifact per task. Keep build logs or a timing trace as sanitized evidence. The graph's `git_head` must equal the task pin and its generation must match `.astria/generation.txt`. Build structural graphs explicitly with `--backend none`; no paid build or agent is required to author/validate this workflow.

Example setup command for an already prepared graph copy:

```powershell
astria run C:/eval/corpora/spend-astria --backend none
```

Use an external timer around the **entire command** when measuring its practical build overhead. If recording native-only time instead, label that boundary in its evidence. Repeat for `watch-astria`. Do not run this command on the main checkout for evaluation preparation.

Each `indexing.artifact` JSON must have the following shape. The numbers must come from retained measurements; omit a measurement or set it to `null` when unknown. `update_seconds` is total measured update overhead since this graph was initially built, not a predicted future cost. A measured zero requires evidence that no update was performed. Binary and graph hashes must be actual 64-character SHA-256 values, not source commit IDs.

```json
{
  "commit": "2509bb184a2bd886a8f0492635ea0556af98643d",
  "astria_binary_path": "C:/eval/runtime/astria.node",
  "astria_binary_sha256": "actual SHA-256 of the native artifact used to build",
  "graph_sha256": "actual SHA-256 of this copy's .astria/graph.json",
  "initial_build_seconds": null,
  "update_seconds": null
}
```

A recorded measurement replaces `null` with `{"value": 1.234, "evidence": ["build-timing.json"]}`. Evidence paths are relative to the indexing artifact and must exist. The runner fingerprints them, rather than guessing elapsed times or using zero for missing data. Evidence should identify the operation, timing boundary, timestamp, commit, binary and actual command. Keep credentials and raw environment dumps out of evidence.

## Supply an agent adapter

`agent.command` is a fixed argv array passed directly to an executable with `shell: false`; no shell command strings or inline interpreter evaluation are accepted. Put model/settings in the manifest and credential values in the inherited environment only. Absolute script paths are recommended because each command runs in its supplied project. The executable and existing absolute file arguments are fingerprinted and checked for changes between conditions. No command changes are allowed between the paired conditions.

The command receives one JSON request on stdin. The same request is saved at `ASTRIA_TASK_REQUEST`; `ASTRIA_TASK_RESULT` identifies where the adapter must write its result. The adapter must honor `model`, `settings`, `project`, `prompt`, `graph_access`, and `allowed_edit_paths`. Baseline must use its normal source-reading tools with graph tools disabled; Astria may additionally use the supplied graph. Apply the same source-read and tool restrictions otherwise. Run each request in a **fresh agent session**, with no history from the other condition. Pin the remote model release when the provider supports it. The harness audits adapter attestations, but cannot independently enforce a remote provider's actual model or the agent's tool permissions.

`codex-adapter.mjs` launches one fresh ephemeral `codex exec` per request with
the exact model and reasoning settings. It ignores user configuration, disables
configured MCP servers and multi-agent execution, disables sandbox network access,
and adds only the run evidence directory to the project's writable paths. Baseline
receives ordinary source-search instructions; Astria also receives the pinned CLI
query instructions. The task's edit scope is prompted and independently audited
by Git. Shell commands remain capable of arbitrary project reads, so graph/read
tool restrictions require trace review; this is not a tool-level access proof.

The adapter records consumed input plus output tokens from completed CLI turn
usage events, including cached input in input totals rather than adding it twice.
Incomplete/failed usage stays unknown. It retains sanitized usage and event types,
runtime/log hashes and the final answer, never raw shell outputs. Source-read
operation counts remain `null`: a shell command transcript cannot establish
instrumented reads. Do not infer source-read savings from missing counts.
`agent.runtime_artifacts` pins the Codex binary, CLI entrypoint and native binding
for both conditions. A usable runtime, model access, authentication, clean external
inputs and independent correctness reviews are required before reporting outcomes.

Request fields include `task_id`, `condition`, `commit`, `result_path`, and `evidence_directory`. The adapter result is:

```json
{
  "model": "gpt-6.1-sol",
  "settings": {"codex_binary": "C:/runtime/codex.exe", "reasoning_effort": "high", "network": false, "sandbox": "workspace-write", "astria_cli": "C:/runtime/dist/index.js"},
  "tokens": null,
  "source_reads": null,
  "answer_evidence": ["answer.md"]
}
```

Use the manifest's **exact** settings object. A measured metric has `{"value": 123, "evidence": ["usage.json"]}`. `tokens` means total consumed input plus output tokens, including tool context and any measured failed calls, not just final-answer tokens. `source_reads` counts instrumented source-file read operations, including repeats; graph responses are not source reads. A trace containing only unique file names cannot substantiate an operation count. Omit unavailable metrics. Sanitize usage/read traces to retain counts, paths, operation IDs and timestamps without source contents, credentials or request headers. The adapter writes a final answer or patch-review artifact for correctness review. It must not put secrets in any artifact.

The runner records monotonic wall time including agent startup, process status, Git changed/untracked paths, patch hash, and wrong-file-edit count using the task's exact allowed paths. Build output ignored by Git is not counted as an edit. It fingerprints stdout/stderr and does not persist their potentially sensitive raw contents. An adapter that exits without valid model/settings attestation cannot support a comparable improvement claim. Audit the adapter and its instrumentation before interpreting results.

## Validate, execute, and review

```powershell
# Read-only validation and execution plan; no agent starts.
node scripts/bench/tasks/run.mjs C:/eval/manifest.json

# Explicitly execute potentially paid agents against the supplied disposable copies.
node scripts/bench/tasks/run.mjs C:/eval/manifest.json --run

# All correctness remains unknown until a reviewer supplies evidence.
node scripts/bench/tasks/report.mjs C:/eval/results/astria-task-evaluation-001/results.json

# Add the independent rubric review to obtain comparable correct-pair metrics.
node scripts/bench/tasks/report.mjs C:/eval/results/astria-task-evaluation-001/results.json C:/eval/reviews.json
```

Output must be a new directory outside all inputs. The runner pre-registers every condition, alternates baseline/Astria order across tasks, and saves progress atomically. Interrupted, timed out, spawn-failed and invalid-evidence entries stay in the results. Do not remove failures or reuse edited task copies for another run. Supply fresh clean copies for every repetition and retain the manifest and run artifacts.

Reviews are separate from the agent result. A human or explicitly configured independent evaluator must assess **every pinned criterion** using the final diff/answer and any compile evidence. The workflow never infers correctness from exit code. Review JSON contains:

```json
{
  "results_sha256": "actual SHA-256 of the completed immutable results.json",
  "entries": [
    {
      "task_id": "failed-run-lifetime-spend",
      "condition": "baseline",
      "rubric_sha256": "copy the pinned rubric hash from results.json",
      "reviewer": "human-review-01",
      "decision": "unknown",
      "criteria": [
        {"id": "include-failed", "status": "unknown"},
        {"id": "exclude-running", "status": "unknown"},
        {"id": "scope-and-comment", "status": "unknown"}
      ],
      "evidence": ["reviews/spend-baseline.md"]
    }
  ]
}
```

Add an entry for each reviewed condition; omissions remain unknown. Decisions are `correct`, `incorrect`, or `unknown`; criterion statuses are `pass`, `fail`, or `unknown`. Correct requires every criterion to pass and retained review evidence. Evidence paths resolve relative to the review JSON. The exact immutable result file and rubric are pinned by hash.

## Interpret the report

The reporter retains the full planned denominator and prints failures, unfinished runs, unknown correctness, and success rates separately by condition. Savings are computed only for pairs whose two conditions completed successfully, retained the pinned HEAD, attested the same model/settings and unchanged runtime, and were explicitly reviewed as correct. Each metric has its own measured-pair count; a missing measurement never becomes zero. Positive savings favor Astria and negative savings favor baseline. File-edit evidence covers tracked and untracked source changes; it does not prove semantic correctness.

For each comparable correct task, cold-task net time is `baseline time - Astria time - initial build time - measured updates`. Reuse break-even is `ceil((initial build + measured updates) / positive task time savings)`. It is unknown when either indexing measurement or comparable task savings is absent, and undefined when Astria saves no time. The value assumes repeated tasks behaving like that specific measured case while reusing the graph; future update costs must be measured and added, not silently assumed free. Build/update timing is excluded from agent wall time and shown explicitly.

Report repetitions and distributions before broad claims. This small diagnostic manifest, serial runs, alternating order, and unflushed filesystem caches provide no statistical significance or universal superiority evidence. Model pricing and dollar savings require separately measured spend; token differences alone are not billing evidence. The reserved Requests/Commander retrieval suites remain untouched until a separately authorized evaluation.
