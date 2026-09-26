// promptfoo provider: answer one question from the astria graph.
// Env: ASTRIA_CMD (default "astria"), BENCH_CORPUS (default cwd).
// promptfoo requires a provider class implementing id() and callApi().
const { spawnSync } = require('node:child_process');

class AstriaProvider {
  id() {
    return 'astria';
  }

  async callApi(prompt) {
    const r = spawnSync(`${process.env.ASTRIA_CMD || 'astria'} query "${prompt}" --budget 4000`, {
      cwd: process.env.BENCH_CORPUS || process.cwd(),
      encoding: 'utf8',
      shell: true,
      timeout: 120_000,
      maxBuffer: 32 * 1024 * 1024,
    });
    return { output: r.stdout || `ERROR: ${r.stderr || String(r.error)}` };
  }
}

module.exports = AstriaProvider;
