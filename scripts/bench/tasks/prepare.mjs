// Explicit disposable corpus preparation. Never changes the source checkout.
import { existsSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { performance } from 'node:perf_hooks';
import { assert, canonical, executable, git, hashFile, overlaps, readJson, save } from './common.mjs';

const [templateFile, sourceArg, destinationArg, cliArg, nativeArg, flag] = process.argv.slice(2);
assert(templateFile && sourceArg && destinationArg && cliArg && nativeArg && flag === '--prepare',
  'Usage: node prepare.mjs template.json SOURCE_REPO NEW_EXTERNAL_DIRECTORY CLI_ENTRYPOINT NATIVE_ARTIFACT --prepare');
const source = canonical(sourceArg), cli = canonical(cliArg), native = canonical(nativeArg);
const repo = canonical(fileURLToPath(new URL('../../../', import.meta.url)));
const destination = path.resolve(destinationArg);
assert(!existsSync(destination), 'Destination must be new');
let parent = path.dirname(destination); while (!existsSync(parent)) parent = path.dirname(parent);
const actual = path.join(canonical(parent), path.relative(parent, destination));
assert(!overlaps(actual, repo) && !overlaps(actual, source), 'Preparation must be outside source/main checkouts');
assert(canonical(git(source, 'rev-parse', '--show-toplevel')) === source, 'Source must be a Git repository root');
assert(path.basename(native) === 'astria.node' && path.dirname(native) === path.dirname(cli), 'Use colocated built CLI and native artifact');
assert(!existsSync(path.join(path.dirname(cli), '../astria.node')), 'Remove ambiguous package-root binding before preparation');
const manifest = readJson(templateFile);
assert(manifest.schema_version === 1 && Array.isArray(manifest.tasks) && manifest.tasks.length, 'Supply a task manifest template');
assert(manifest.agent?.settings, 'Supply Codex settings');
const codex = executable(manifest.agent.settings.codex_binary);
// Validate task identifiers and commits before creating any directories.
const ids = new Set();
for (const task of manifest.tasks) {
  assert(/^[a-z0-9][a-z0-9-]*$/.test(task.id) && !ids.has(task.id), 'Unique safe task IDs required'); ids.add(task.id);
  assert(/^[0-9a-f]{40}$/.test(task.commit) && git(source, 'rev-parse', `${task.commit}^{commit}`) === task.commit, 'Task commit must exist in the supplied source');
  for (const anchor of task.grounding ?? []) assert(git(source, 'show', `${task.commit}:${anchor.path}`).includes(anchor.contains), 'Pinned task grounding is missing');
}
const command = (binary, argv, cwd) => {
  const result = spawnSync(binary, argv, { cwd, shell: false, encoding: 'utf8', timeout: 30 * 60 * 1000, maxBuffer: 32 * 1024 * 1024 });
  assert(!result.error && result.status === 0, `Preparation command failed: ${path.basename(binary)} ${argv[0]} (raw logs withheld)`);
  return result;
};
mkdirSync(destination, { recursive: true });
const cliHash = hashFile(cli), nativeHash = hashFile(native);
for (const task of manifest.tasks) {
  task.projects = {};
  for (const condition of ['baseline', 'astria']) {
    const root = path.join(destination, `${task.id}-${condition}`);
    command('git', ['clone', '--no-hardlinks', '--no-checkout', '--', source, root]);
    // This is a newly created clone with no work to discard, never a shared copy.
    command('git', ['-C', root, 'switch', '--detach', task.commit]);
    assert(git(root, 'rev-parse', 'HEAD') === task.commit, 'Clone pin mismatch');
    task.projects[condition] = root;
    if (condition === 'baseline') assert(!existsSync(path.join(root, '.astria')), 'Pinned source unexpectedly tracks graph artifacts');
    else {
      const start = performance.now();
      command(process.execPath, [cli, 'run', root, '--backend', 'none'], root);
      const elapsed = (performance.now() - start) / 1000;
      assert(hashFile(cli) === cliHash && hashFile(native) === nativeHash, 'Runtime changed during preparation');
      const indexDir = path.join(destination, 'indexing', task.id); mkdirSync(indexDir, { recursive: true });
      save(path.join(indexDir, 'timing.json'), { operation: 'initial-structural-build', commit: task.commit,
        command: [process.execPath, cli, 'run', root, '--backend', 'none'], boundary: 'whole CLI process, monotonic clock',
        initial_build_seconds: elapsed, update_operations: 0, update_seconds: 0,
        cli_sha256: cliHash, native_sha256: nativeHash, recorded_at: new Date().toISOString() });
      const artifact = path.join(indexDir, 'indexing.json');
      save(artifact, { commit: task.commit, astria_binary_path: native, astria_binary_sha256: nativeHash,
        astria_cli_path: cli, astria_cli_sha256: cliHash, graph_sha256: hashFile(path.join(root, '.astria/graph.json')),
        initial_build_seconds: { value: elapsed, evidence: ['timing.json'] }, update_seconds: { value: 0, evidence: ['timing.json'] } });
      task.indexing = { artifact };
    }
  }
}
manifest.output = path.join(destination, 'results');
manifest.agent.command = [process.execPath, fileURLToPath(new URL('./codex-adapter.mjs', import.meta.url))];
manifest.agent.settings.astria_cli = cli;
manifest.agent.settings.codex_binary = codex;
manifest.agent.runtime_artifacts = [codex, cli, native];
save(path.join(destination, 'manifest.json'), manifest);
console.log(`Prepared inputs and measured structural indexing: ${path.join(destination, 'manifest.json')}. No agent was started.`);
