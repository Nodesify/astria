// promptfoo provider: answer one question from the ORIGINAL Python graphify.
// Env: GRAPHIFY_CMD (default "graphify"), BENCH_CORPUS (default cwd).
// The pinned benchmark corpus runs upstream @91f4d12; if that build has no
// `query` command, point GRAPHIFY_CMD at a newer graphify install and run it
// on the same corpus - the comparison stays apples-to-apples as long as both
// providers answer the same questions on the same graph.
const { spawnSync } = require('node:child_process');

class GraphifyProvider {
  id() {
    return 'graphify';
  }

  async callApi(prompt) {
    // Symmetric budgets: GRAPHIFY_BUDGET should match ASTRIA_BUDGET. The
    // graph flag goes after the question — graphify parses options following
    // the free-text query ("graphify query \"<q>\" [--budget N] [--graph p]").
    const extra = [
      process.env.GRAPHIFY_BUDGET ? `--budget ${process.env.GRAPHIFY_BUDGET}` : '',
      process.env.GRAPHIFY_GRAPH ? `--graph "${process.env.GRAPHIFY_GRAPH}"` : '',
    ]
      .filter(Boolean)
      .join(' ');
    const r = spawnSync(`${process.env.GRAPHIFY_CMD || 'graphify'} query "${prompt}" ${extra}`, {
      cwd: process.env.BENCH_CORPUS || process.cwd(),
      encoding: 'utf8',
      shell: true,
      timeout: 300_000,
      maxBuffer: 32 * 1024 * 1024,
    });
    return { output: r.stdout || `ERROR: ${r.stderr || String(r.error)}` };
  }
}

module.exports = GraphifyProvider;
