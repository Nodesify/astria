import { healthReport } from '../native';

export async function healthCommand(opts: {
  graph: string;
  json?: boolean;
  minScore?: string;
}) {
  const report = healthReport(opts.graph);
  if (opts.json) {
    console.log(
      JSON.stringify(
        {
          score: report.score,
          grade: report.grade,
          ageDays: report.ageDays,
          nodeCount: report.nodeCount,
          edgeCount: report.edgeCount,
          deadCode: report.deadCode,
          cycles: report.cycles,
          hubs: report.hubs,
        },
        null,
        2,
      ),
    );
  } else {
    console.log(report.text);
  }

  // CI gate: `astria health --min-score 70` must be able to fail a build.
  // Without this the command always exited 0, so the obvious threshold gate
  // silently never fired. The score is checked after the report is printed, so
  // a failing gate still shows the operator why.
  if (opts.minScore !== undefined) {
    const min = Number(opts.minScore);
    if (!Number.isFinite(min)) {
      console.error(`Error: --min-score must be a number, got "${opts.minScore}"`);
      process.exitCode = 1;
      return;
    }
    if (report.score < min) {
      console.error(
        `\nHealth score ${report.score} is below the required minimum of ${min}.`,
      );
      process.exitCode = 1;
    }
  }
}
