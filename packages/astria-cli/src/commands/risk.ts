import { riskReport } from '../native';

export async function riskCommand(opts: { graph: string; staged?: boolean; json?: boolean }) {
  try {
    const report = riskReport(opts.graph, opts.staged === true);
    if (opts.json) {
      console.log(
        JSON.stringify(
          {
            score: report.score,
            level: report.level,
            changedFiles: report.changedFiles,
            filesWithSymbols: report.filesWithSymbols,
            impacted: report.impacted,
            byDepth: report.byDepth,
            communities: report.communities,
            entries: report.entries,
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
