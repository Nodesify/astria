import { existsSync, mkdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { performance } from 'node:perf_hooks';
import { assert, canonical, dirty, evidence, executable, git, hashFile, measurement, overlaps, readJson, save, sha } from './common.mjs';

const args = process.argv.slice(2);
assert(args[0] && args.slice(1).every(arg => arg === '--run'), 'Usage: node scripts/bench/tasks/run.mjs manifest.json [--run]');
const execute = args.includes('--run');
const manifestFile = path.resolve(args[0]);
const base = path.dirname(manifestFile);
const manifest = readJson(manifestFile);
assert(manifest.schema_version === 1 && Array.isArray(manifest.tasks) && manifest.tasks.length > 0, 'Expected schema_version 1 and nonempty tasks');
const repo = canonical(fileURLToPath(new URL('../../../', import.meta.url)));
const agent = manifest.agent;
assert(agent && typeof agent.model === 'string' && agent.model && agent.settings && Array.isArray(agent.command) && agent.command.length > 0, 'Supply agent model, identical settings, and a fixed command argv');
assert(agent.command.every(arg => typeof arg === 'string' && !/[\r\n]/.test(arg)), 'Command argv must be plain strings');
assert(!/^(?:cmd|powershell|pwsh|bash|sh)(?:\.exe)?$/i.test(path.basename(agent.command[0])) && !/\.(?:cmd|bat)$/i.test(agent.command[0]), 'Use a direct executable, not a shell');
assert(!agent.command.some(arg => /^(?:-e|-c|--eval|-command)$/i.test(arg)), 'Inline command strings are unsupported; supply an adapter file');
assert(!/(?:api[_-]?key|password|secret|authorization|bearer|credential)/i.test(JSON.stringify(agent)), 'Do not put credentials in agent command/settings; inherit credential environment variables');
assert(Number.isFinite(manifest.timeout_seconds) && manifest.timeout_seconds > 0, 'Supply a positive timeout_seconds');
const agentExecutable = executable(agent.command[0]);
const agentArtifacts = [agentExecutable, ...agent.command.slice(1).filter(arg => path.isAbsolute(arg) && existsSync(arg) && statSync(arg).isFile())]
  .map(file => ({ path: canonical(file), sha256: hashFile(file) }));
const roots = [];
const ids = new Set();
const prepared = manifest.tasks.map(task => {
  assert(/^[a-z0-9][a-z0-9-]*$/.test(task.id) && !ids.has(task.id), 'Task IDs must be unique safe names'); ids.add(task.id);
  assert(/^(?:[0-9a-f]{40}|[0-9a-f]{64})$/.test(task.commit), `Full commit pin required: ${task.id}`);
  assert(typeof task.prompt === 'string' && task.prompt.length > 0 && Array.isArray(task.rubric) && task.rubric.length > 0, 'Supply a concrete task and explicit rubric');
  assert(task.rubric.every(item => item.id && item.criterion) && new Set(task.rubric.map(item => item.id)).size === task.rubric.length, 'Rubric IDs must be unique');
  assert(Array.isArray(task.allowed_edit_paths), 'Supply allowed_edit_paths (empty for read-only tasks)');
  const pair = {};
  for (const condition of ['baseline', 'astria']) {
    assert(typeof task.projects?.[condition] === 'string', `Supply ${condition} project path`);
    const root = canonical(path.resolve(base, task.projects[condition]));
    assert(!overlaps(root, repo), 'Evaluation projects must not overlap the main checkout');
    assert(!roots.some(other => overlaps(root, other)), 'Every task condition requires a separate non-overlapping project copy'); roots.push(root);
    assert(canonical(git(root, 'rev-parse', '--show-toplevel')) === root, 'Project must be a Git repository root');
    assert(git(root, 'rev-parse', 'HEAD') === task.commit && dirty(root).length === 0, 'Input must be clean at its pinned commit');
    for (const grounding of task.grounding ?? []) {
      const result = spawnSync('git', ['-C', root, 'show', `${task.commit}:${grounding.path}`], { encoding: 'utf8', shell: false });
      assert(result.status === 0 && result.stdout.includes(grounding.contains), `Task grounding failed: ${grounding.path}`);
    }
    pair[condition] = { root, commit: task.commit };
    if (condition === 'baseline') assert(!existsSync(path.join(root, '.astria')) && !existsSync(path.join(root, '.graphify')), 'Baseline must not contain graph artifacts');
    else {
      const graphPath = path.join(root, '.astria/graph.json');
      const graph = readJson(graphPath);
      const stamp = readFileSync(path.join(root, '.astria/generation.txt'), 'utf8').trim();
      assert(graph._meta?.git_head === task.commit && graph._meta?.graph_generation === stamp, 'Astria graph must match pinned input commit and publication generation');
      pair[condition].graph = { generation: stamp, sha256: hashFile(graphPath), report_sha256: hashFile(path.join(root, '.astria/graph_report.md')) };
    }
  }
  assert(task.indexing && typeof task.indexing.artifact === 'string', 'Supply indexing provenance artifact path');
  const indexingFile = path.resolve(base, task.indexing.artifact);
  const indexing = readJson(indexingFile);
  assert(indexing.commit === task.commit && indexing.graph_sha256 === pair.astria.graph.sha256 && indexing.astria_binary_sha256?.match(/^[0-9a-f]{64}$/), 'Indexing provenance must pin corpus, actual graph, and Astria binary');
  assert(typeof indexing.astria_binary_path === 'string' && hashFile(path.resolve(path.dirname(indexingFile), indexing.astria_binary_path)) === indexing.astria_binary_sha256, 'Indexing artifact must identify the actual fingerprinted Astria binary');
  const indexRoot = path.dirname(indexingFile);
  pair.astria.indexing = { artifact_path: indexingFile, artifact_sha256: hashFile(indexingFile), astria_binary_sha256: indexing.astria_binary_sha256,
    initial_build_seconds: measurement(indexRoot, indexing.initial_build_seconds), update_seconds: measurement(indexRoot, indexing.update_seconds) };
  return { ...task, projects: pair, rubric_sha256: sha(JSON.stringify(task.rubric)) };
});
const plan = { schema_version: 1, manifest_sha256: hashFile(manifestFile), agent, agent_artifacts: agentArtifacts,
  tasks: prepared.map(t => ({ id: t.id, commit: t.commit, projects: t.projects, rubric: t.rubric, rubric_sha256: t.rubric_sha256 })) };
if (!execute) {
  console.log(JSON.stringify({ mode: 'validation-only', ...plan, note: 'No agent started; add --run to execute the paired tasks.' }, null, 2));
} else {
  assert(typeof manifest.output === 'string', 'Supply a new output directory');
  const output = path.resolve(base, manifest.output);
  assert(!existsSync(output), 'Output directory must be new');
  // Resolve existing ancestors so symlinks cannot redirect artifacts into a corpus.
  let parent = path.dirname(output); while (!existsSync(parent)) parent = path.dirname(parent);
  const actualOutput = path.join(canonical(parent), path.relative(parent, output));
  assert(!roots.some(root => overlaps(root, actualOutput)), 'Output must be outside all evaluation projects');
  mkdirSync(output, { recursive: true });
  const resultFile = path.join(output, 'results.json');
  const runs = prepared.flatMap((task, index) => (index % 2 ? ['astria', 'baseline'] : ['baseline', 'astria']).map(condition => ({ task_id: task.id, condition, status: 'not-started', correctness: 'unreviewed' })));
  const results = { ...plan, started_at: new Date().toISOString(), runner_sha256: hashFile(fileURLToPath(import.meta.url)), node: process.version, platform: process.platform, runs };
  save(resultFile, results);
  for (const row of runs) {
    const task = prepared.find(t => t.id === row.task_id), project = task.projects[row.condition];
    const runDir = path.join(output, `${task.id}-${row.condition}`); mkdirSync(runDir);
    row.status = 'running'; save(resultFile, results);
    const request = { schema_version: 1, task_id: task.id, condition: row.condition, project: project.root, commit: task.commit,
      model: agent.model, settings: agent.settings, prompt: task.prompt, allowed_edit_paths: task.allowed_edit_paths,
      graph_access: row.condition === 'astria', result_path: path.join(runDir, 'agent-result.json'), evidence_directory: runDir };
    save(path.join(runDir, 'request.json'), request);
    try {
      assert(git(project.root, 'rev-parse', 'HEAD') === task.commit && dirty(project.root).length === 0, 'Project changed before execution');
      assert(agentArtifacts.every(file => hashFile(file.path) === file.sha256), 'Agent executable or adapter changed between conditions');
      const start = performance.now();
      const processResult = spawnSync(agentExecutable, agent.command.slice(1), { cwd: project.root, shell: false,
        input: JSON.stringify(request), encoding: 'utf8', timeout: manifest.timeout_seconds * 1000, maxBuffer: 32 * 1024 * 1024,
        env: { ...process.env, ASTRIA_TASK_REQUEST: path.join(runDir, 'request.json'), ASTRIA_TASK_RESULT: request.result_path } });
      row.elapsed_seconds = { value: (performance.now() - start) / 1000, evidence: 'runner-monotonic-clock' };
      row.exit_code = processResult.status; row.signal = processResult.signal;
      // Preserve output fingerprints, never raw potentially credential-bearing logs.
      row.output = { stdout_sha256: sha(processResult.stdout ?? ''), stderr_sha256: sha(processResult.stderr ?? '') };
      row.status = processResult.error ? (processResult.error.code === 'ETIMEDOUT' ? 'timeout' : 'spawn-failed') : processResult.status === 0 ? 'completed' : 'agent-failed';
      row.agent_error_code = processResult.error?.code ?? null;
      row.post_commit = git(project.root, 'rev-parse', 'HEAD');
      row.changed_paths = git(project.root, 'diff', '--name-only', task.commit).split('\n').filter(Boolean);
      row.untracked_paths = git(project.root, 'ls-files', '--others', '--exclude-standard').split('\n').filter(file => file && !file.startsWith('.astria/'));
      row.wrong_file_edits = [...new Set([...row.changed_paths, ...row.untracked_paths])].filter(file => !task.allowed_edit_paths.includes(file));
      row.wrong_file_edit_count = { value: row.wrong_file_edits.length, evidence: 'git-diff-and-untracked-paths' };
      row.patch_sha256 = sha(git(project.root, 'diff', task.commit));
      row.runtime_verified = agentArtifacts.every(file => hashFile(file.path) === file.sha256);
      if (existsSync(request.result_path)) {
        const supplied = readJson(request.result_path);
        assert(supplied.model === agent.model && sha(JSON.stringify(supplied.settings)) === sha(JSON.stringify(agent.settings)), 'Adapter must attest exact model/settings');
        row.model_verified = true;
        row.tokens = measurement(runDir, supplied.tokens);
        row.source_reads = measurement(runDir, supplied.source_reads);
        row.answer_evidence = evidence(runDir, supplied.answer_evidence);
      }
    } catch (error) {
      row.status = 'invalid-evidence';
      // Error labels are controlled by this runner; external output is hashed.
      row.error = 'input-or-evidence-validation-failed';
    }
    row.tokens ??= { value: null, evidence: null }; row.source_reads ??= { value: null, evidence: null };
    save(resultFile, results);
  }
  results.finished_at = new Date().toISOString(); save(resultFile, results);
  console.log(`Recorded ${runs.length} attempted conditions: ${resultFile}`);
}
