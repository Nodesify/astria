// promptfoo provider: answer one question from the ORIGINAL Python graphify.
// Env: GRAPHIFY_CMD (default "graphify"), BENCH_CORPUS (default cwd).
//
// The pinned benchmark corpus runs upstream @91f4d12; if that build has no
// `query` command, point GRAPHIFY_CMD at a newer graphify install and run it
// on the same corpus — the comparison stays apples-to-apples as long as both
// providers answer the same questions on the same graph.
import { spawnSync } from 'node:child_process';

export default function (prompt) {
  const cli = process.env.GRAPHIFY_CMD || 'graphify';
  const r = spawnSync(`${cli} query "${prompt}"`, {
    cwd: process.env.BENCH_CORPUS || process.cwd(),
    encoding: 'utf8',
    shell: true,
    timeout: 300_000,
    maxBuffer: 32 * 1024 * 1024,
  });
  return r.stdout || `ERROR: ${r.stderr || String(r.error)}`;
}
