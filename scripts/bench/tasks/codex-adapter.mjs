// Starts a fresh Codex exec process only when the paired runner explicitly runs it.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { assert, executable, hashFile, save, sha } from './common.mjs';

const request = JSON.parse(readFileSync(0, 'utf8'));
const settings = request.settings;
const keys = ['codex_binary', 'reasoning_effort', 'network', 'sandbox', 'astria_cli'];
assert(settings && Object.keys(settings).length === keys.length && keys.every(k => Object.hasOwn(settings, k)), 'Codex adapter supports exactly the documented settings');
assert(settings.network === false && settings.sandbox === 'workspace-write', 'Adapter requires network disabled and workspace-write sandbox');
assert(['low', 'medium', 'high', 'xhigh'].includes(settings.reasoning_effort), 'Unsupported reasoning effort');
assert(typeof settings.astria_cli === 'string' && path.isAbsolute(settings.astria_cli), 'Pin an absolute Astria CLI entrypoint');
const binary = executable(settings.codex_binary);
const runDir = request.evidence_directory;
const answer = path.join(runDir, 'answer.md');
const permissions = request.graph_access
  ? `You may use the existing .astria graph via: node ${JSON.stringify(settings.astria_cli)} query <question>, explain <node>, or affected <node>. Do not build or update the graph during this task. Graph access is the experimental treatment.`
  : 'Use ordinary source reads and searches. Do not use Astria, Graphify, knowledge graphs, or graph tools. This project intentionally has no graph artifacts.';
const prompt = `${request.prompt}\n\nEvaluation rules: ${permissions}\nEdit only these exact paths: ${JSON.stringify(request.allowed_edit_paths)}. Never commit, install dependencies, contact the network, use subagents, or execute tests. Do not read credentials or configuration outside the project. A compile check is permitted. Explain the final change and any limitations. Do not modify AGENTS.md or evaluation artifacts.`;
const argv = ['exec', '--ignore-user-config', '--ephemeral', '--json', '--color', 'never',
  '--model', request.model, '--sandbox', settings.sandbox, '--cd', request.project,
  '--add-dir', runDir, '--config', `model_reasoning_effort=${JSON.stringify(settings.reasoning_effort)}`,
  '--config', 'sandbox_workspace_write.network_access=false', '--config', 'mcp_servers={}',
  '--config', 'features.multi_agent=false', '--output-last-message', answer, '-'];
const version = spawnSync(binary, ['--version'], { encoding: 'utf8', shell: false });
assert(version.status === 0, 'Codex runtime unavailable');
assert(Number.isFinite(request.timeout_seconds) && request.timeout_seconds > 5, 'Supply a timeout exceeding five seconds');
const execution = spawnSync(binary, argv, { input: prompt, encoding: 'utf8', shell: false,
  timeout: (request.timeout_seconds - 5) * 1000, maxBuffer: 32 * 1024 * 1024 });
// Do not retain tool bodies or raw logs, which can include source or credentials.
const events = (execution.stdout ?? '').split(/\r?\n/).filter(Boolean).flatMap(line => {
  try { return [JSON.parse(line)]; } catch { return []; }
});
const completions = events.filter(event => event.type === 'turn.completed');
const failed = events.some(event => event.type === 'turn.failed' || event.type === 'error');
const usage = completions.map(event => event.usage);
const known = !failed && execution.status === 0 && usage.length > 0 && usage.every(u =>
  Number.isFinite(u?.input_tokens) && u.input_tokens >= 0 && Number.isFinite(u?.output_tokens) && u.output_tokens >= 0);
const trace = { schema_version: 1, codex_version: version.stdout.trim(), codex_binary_sha256: hashFile(binary),
  astria_cli_sha256: hashFile(settings.astria_cli), model: request.model, settings,
  exit_code: execution.status, stdout_sha256: sha(execution.stdout ?? ''), stderr_sha256: sha(execution.stderr ?? ''),
  usage: usage.map(u => ({ input_tokens: u?.input_tokens ?? null, cached_input_tokens: u?.cached_input_tokens ?? null, output_tokens: u?.output_tokens ?? null })),
  event_types: events.map(event => event.type), tokens_complete: known,
  source_reads: null, source_read_limit: 'CLI shell commands are not instrumented file-read operations' };
save(path.join(runDir, 'codex-usage.json'), trace);
save(request.result_path, { model: request.model, settings,
  tokens: known ? { value: usage.reduce((sum, u) => sum + u.input_tokens + u.output_tokens, 0), evidence: ['codex-usage.json'] } : null,
  source_reads: null, answer_evidence: execution.status === 0 ? ['answer.md'] : null });
if (execution.error || execution.status !== 0 || failed) process.exitCode = 1;
