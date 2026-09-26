// promptfoo provider: answer one question from the astria graph.
// Env: ASTRIA_CMD (default "astria"), BENCH_CORPUS (default cwd).
import { spawnSync } from 'node:child_process';

export default function (prompt) {
  const cli = process.env.ASTRIA_CMD || 'astria';
  const r = spawnSync(`${cli} query "${prompt}" --budget 4000`, {
    cwd: process.env.BENCH_CORPUS || process.cwd(),
    encoding: 'utf8',
    shell: true,
    timeout: 120_000,
    maxBuffer: 32 * 1024 * 1024,
  });
  return r.stdout || `ERROR: ${r.stderr || String(r.error)}`;
}
