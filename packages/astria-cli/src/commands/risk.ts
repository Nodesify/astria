import { changeReview, ReviewOptions } from './change-review';

export async function riskCommand(opts: ReviewOptions & { json?: boolean }) {
  try {
    const report = changeReview(opts);
    if (opts.json) {
      console.log(JSON.stringify(report, null, 2));
    } else {
      console.log(report.text);
    }
    if (!report.coverageComplete) process.exitCode = 1;
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
