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
import { graphBuildInfo, healthReport, verifySourceCommit } from '../native';
import { changeReview, ChangeReview } from './change-review';

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
  /** Score the committed diff `base...head` instead of the working tree (CI/PR mode). */
  base?: string;
  /** Head ref of the range; defaults to HEAD when --base is given. */
  head?: string;
  json?: boolean;
}

interface Check {
  name: string;
  passed: boolean;
  detail: string;
}

// Git work-tree detection that distinguishes the three outcomes: inside a
// work tree, genuinely outside one (exit 128 + "not a git repository"), or
// the git invocation FAILED (git missing, broken index, ...). A failure is
// an error string — never a silent "not a repository" skip, which would
// let the gate pass without the diff-risk check it promised.
function gitWorkTreeStatus(projectRoot: string): { inside: boolean; error: string | null } {
  try {
    const out = execFileSync('git', ['rev-parse', '--is-inside-work-tree'], {
      cwd: projectRoot,
      encoding: 'utf8',
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    return { inside: out.trim() === 'true', error: null };
  } catch (e: any) {
    const stderr = String(e?.stderr ?? '');
    if (e?.status === 128 && /not a git repository/i.test(stderr)) {
      return { inside: false, error: null };
    }
    if (e?.code === 'ENOENT') {
      return { inside: false, error: 'git executable not found on PATH' };
    }
    return {
      inside: false,
      error: (stderr || e?.message || String(e)).trim().split('\n').pop() || 'git failed',
    };
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
    let review: ChangeReview | null = null;
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

    // 2. Two distinct freshness signals:
    //    - graph AGE (how long since publication, ceiling-configured), and
    //    - source COVERAGE (does the graph represent the current HEAD —
    //      commit identity plus manifest content comparison).
    //    - extraction RULESET (was the graph built by the current extraction
    //      code at all — see check 2b).
    //    A graph built after a commit can still fail coverage: it may have
    //    been built from a dirty tree, an older commit, or drifted since.
    // One probe for both coverage and diff-risk.
    const gitStatus = gitWorkTreeStatus(opts.graph);
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
          : `graph is ${ageHours.toFixed(1)} h old (age limit ${maxAgeHours} h; source coverage is a separate check below)`,
      });

      // 2b. Extraction ruleset. Age and HEAD coverage both pass for a graph
      //     the CURRENT build would extract differently: the old cache is
      //     reused, so the graph reports yesterday's facts with today's
      //     timestamps. The committed self-graph drifted five commits this
      //     way (`extraction_hash_version` v12 against code at v14). A version
      //     mismatch means the recorded facts predate the current extraction
      //     rules, so nodes and edges may be missing; rebuild before trusting
      //     a merge decision to them.
      const builtWith = build?.extractionHashVersion as string | undefined;
      const currentRules = build?.currentExtractionHashVersion as string | undefined;
      const extractionCurrent =
        !!builtWith && !!currentRules && builtWith === currentRules;
      checks.push({
        name: 'extraction-current',
        passed: extractionCurrent,
        detail: extractionCurrent
          ? `graph extracted by the current ruleset (${builtWith})`
          : `graph was extracted with ${builtWith ?? 'an unknown version'} but this build extracts with ${currentRules ?? '?'} — run 'astria update .' to re-extract changed files`,
      });

      // Coverage: the HEAD recorded at publication must equal the current
      // HEAD, and every manifest file must still hash to the content the
      // graph was built from. A publish timestamp alone only proved the
      // build happened LATER — never that it saw the commit.
      if (gitStatus.error !== null) {
        checks.push({
          name: 'graph-covers-head',
          passed: false,
          detail: `git detection failed: ${gitStatus.error} — cannot verify the graph covers HEAD`,
        });
      } else if (gitStatus.inside) {
        try {
          const coverage = verifySourceCommit(opts.graph);
          const samples = coverage.driftSamples.length > 0
            ? ` (${coverage.driftSamples.join('; ')})`
            : '';
          const verified = coverage.filesChecked > 0
            ? ` — ${coverage.filesChecked} manifest file(s) content-verified`
            : '';
          checks.push({
            name: 'graph-covers-head',
            passed: coverage.coversHead === true,
            detail: `${coverage.reason}${verified}${samples}`,
          });
        } catch (e: any) {
          checks.push({
            name: 'graph-covers-head',
            passed: false,
            detail: `coverage verification failed: ${e?.message || e}`,
          });
        }
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

      // 4. Blast radius of the pending diff. Outside a git repository is an
      //    explicit, visible skip; a FAILED git detection is a FAILED check
      //    — never a silent "not a repository" skip that lets the gate pass
      //    without the diff-risk it promised.
      if (gitStatus.error !== null) {
        checks.push({
          name: 'diff-risk',
          passed: false,
          detail: `git detection failed: ${gitStatus.error} — cannot score the diff`,
        });
      } else if (!gitStatus.inside) {
        checks.push({
          name: 'diff-risk',
          passed: true,
          detail: 'skipped — not a git repository (risk requires a diff to score)',
        });
      } else {
        try {
          review = changeReview(opts);
          const riskPassed = review.coverageComplete && review.score !== null && review.score <= maxRisk;
          checks.push({
            name: 'diff-risk',
            passed: riskPassed,
            detail: review.coverageComplete
              ? `change risk ${review.score}/100 (ceiling ${maxRisk}); ${review.impacted} surviving consumers, ${review.directConsumers} direct, ${review.testConsumers} tests; base ${review.baseCommit}, head ${review.headCommit ?? review.afterIdentity}`
              : `coverage incomplete — risk is unknown: ${review.coverageIssues.slice(0, 5).join('; ')}`,
          });
        } catch (e: any) {
          checks.push({
            name: 'diff-risk',
            passed: false,
            detail: `risk calculation failed: ${e?.message || e}`,
          });
        }
      }
    }

    const passed = checks.every((c) => c.passed);

    if (opts.json) {
      console.log(JSON.stringify({ passed, checks, review }, null, 2));
    } else {
      console.log(`${BOLD}Merge gate — ${passed ? 'PASSED ✓' : 'FAILED ✗'}${RESET}\n`);
      for (const check of checks) {
        const mark = check.passed ? `${GREEN}✓${RESET}` : `${RED}✗${RESET}`;
        console.log(` ${mark} ${check.name}: ${check.detail}`);
      }
      if (review) console.log(`\n${review.text}`);
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
