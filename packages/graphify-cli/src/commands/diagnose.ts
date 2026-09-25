import { diagnoseGraph } from '../native';

export async function diagnoseCommand(opts: { graph: string; json?: boolean }) {
  try {
    const report = diagnoseGraph(opts.graph);
    if (opts.json) {
      const { text: _text, ...rest } = report;
      console.log(JSON.stringify(rest, null, 2));
      return;
    }
    console.log(report.text);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
