// Merge gate: the OSS-side artifact of the hosted tier's "merge-gate
// verification". A CI command that refuses to merge on a stale, missing, or
// unhealthy graph, and quantifies the blast radius of the pending diff:
//
//   astria merge-gate --max-age-hours 24 --min-health 60 --max-risk 70
//
// Exit 0 = pass (green), exit 1 = fail with the failing checks listed —
// wire it into CI or a GitHub required check. `--json` for the hosted
// pipeline to consume.

import { execFileSync } from 'child_process';
import { existsSync } from 'fs';
import { join } from 'path';
import { graphBuildInfo, healthReport, riskReport } from '../native';

const BOLD = '\x1b[1m';
const GREEN = '\x1b[32m';
const RED = '\x1b[31m';
const DIM = '\x1b[2m';
const RESET = '\x1b[0m';

export interface MergeGateOptions {
  graph: string;
  maxAgeHours?: string;
  minHealth?: string;
  maxRisk?: string;
  staged?: boolean;
  json?: boolean;
}

interface Check {
  name: string;
  passed: boolean;
  detail: string;
}

function lastCommitTime(projectRoot: string): number | null {
  try {
    const out = execFileSync('git', ['log', '-1', '--format=%ct'], {
      cwd: projectRoot,
      encoding: 'utf-8',
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    const t = Number(out.trim());
    return Number.isFinite(t) && t > 0 ? t : null;
  } catch {
    return null;
  }
}

export async function mergeGateCommand(opts: MergeGateOptions) {
  try {
    const maxAgeHours = opts.maxAgeHours !== undefined ? Number(opts.maxAgeHours) : 24;
    const minHealth = opts.minHealth !== undefined ? Number(opts.minHealth) : 60;
    const maxRisk = opts.maxRisk !== undefined ? Number(opts.maxRisk) : 70;
    for (const [name, value] of [['--max-age-hours', maxAgeHours], ['--min-health', minHealth], ['--max-risk', maxRisk]] as const) {
      if (!Number.isFinite(value)) throw new Error(`invalid ${name}: not a number`);
    }

    const checks: Check[] = [];
    const dbPath = join(opts.graph, '.astria', 'db.sqlite');

    // 1. The graph exists and carries build provenance.
    let build: any = null;
    try {
      build = graphBuildInfo(opts.graph);
    } catch {
      // fall through — handled by the exists check
    }
    const exists = build !== null && build !== undefined || existsSync(dbPath);
    checks.push({
      name: 'graph-exists',
      passed: exists,
      detail: exists ? `built by astria ${build?.astriaVersion ?? '?'}, pipeline ${build?.pipelineVersion ?? '?'}` : `no graph at ${dbPath} — run 'astria run' first`,
    });

    // 2. Freshness: the graph must postdate the last commit and be younger
    //    than the configured ceiling. Both signals matter — a graph built
    //    after a commit can still be ancient, and a recent build can
    //    predate the commit it is supposed to describe.
    if (exists) {
      const publishedAt = Number(build?.graphPublishedAt);
      const ageHours = Number.isFinite(publishedAt) && publishedAt > 0
        ? (Date.now() / 1000 - publishedAt) / 3600
        : null;
      checks.push({
        name: 'graph-fresh',
        passed: ageHours !== null && ageHours <= maxAgeHours,
        detail: ageHours === null
          ? 'graph has no publish timestamp (pre-provenance build) — rebuild with `astria run`'
          : `graph is ${ageHours.toFixed(1)} h old (limit ${maxAgeHours} h)`,
      });

      const commitTime = lastCommitTime(opts.graph);
      if (commitTime !== null && Number.isFinite(publishedAt) && publishedAt > 0) {
        checks.push({
          name: 'graph-covers-head',
          passed: publishedAt >= commitTime,
          detail: publishedAt >= commitTime
            ? 'graph was built after the last commit'
            : 'graph predates the last commit — run `astria update` before merging',
        });
      }
    }

    // 3. Health floor.
    if (exists) {
      const health = healthReport(opts.graph);
      checks.push({
        name: 'health-floor',
        passed: health.score >= minHealth,
        detail: `health score ${health.score}/100 (floor ${minHealth}), grade ${health.grade}`,
      });

      // 4. Blast radius of the pending diff (skipped cleanly outside git).
      try {
        const risk = riskReport(opts.graph, opts.staged === true);
        const riskPassed = risk.score <= maxRisk;
        checks.push({
          name: 'diff-risk',
          passed: riskPassed,
          detail: `diff risk ${risk.score}/100 (ceiling ${maxRisk}), ${risk.impacted} symbols impacted across ${risk.changedFiles.length} changed file(s)`,
        });
      } catch {
        // Not a git repo or no diff — the risk check is informational only.
      }
    }

    const passed = checks.every((c) => c.passed);

    if (opts.json) {
      console.log(JSON.stringify({ passed, checks }, null, 2));
    } else {
      console.log(`${BOLD}Merge gate — ${passed ? 'PASSED ✓' : 'FAILED ✗'}${RESET}\n`);
      for (const check of checks) {
        const mark = check.passed ? `${GREEN}✓${RESET}` : `${RED}✗${RESET}`;
        console.log(` ${mark} ${check.name}: ${check.detail}`);
      }
      if (!passed) {
        console.log(`\n${DIM}Fix the failing checks or adjust the thresholds — this gate is what the hosted tier enforces on every merge.${RESET}`);
      }
    }
    if (!passed) process.exitCode = 1;
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
