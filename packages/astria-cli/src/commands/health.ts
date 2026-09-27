import { healthReport } from '../native';

export async function healthCommand(opts: { graph: string; json?: boolean }) {
  try {
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
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
