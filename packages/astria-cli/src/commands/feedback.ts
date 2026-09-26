import { saveQueryResult, reflectGraph } from '../native';

export async function saveResultCommand(
  question: string,
  opts: {
    graph: string;
    answer?: string;
    answerFile?: string;
    outcome?: string;
    correction?: string;
    nodes?: string;
  },
) {
  try {
    let answer = opts.answer;
    if (!answer && opts.answerFile) {
      answer = require('fs').readFileSync(opts.answerFile, 'utf-8');
    }
    if (!answer) {
      console.error('Error: provide --answer or --answer-file');
      process.exitCode = 1;
      return;
    }
    const sourceNodes = (opts.nodes || '')
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);
    const saved = saveQueryResult(
      opts.graph,
      question,
      answer,
      opts.outcome,
      opts.correction,
      sourceNodes.length > 0 ? sourceNodes : undefined
    );
    console.log(`Memory saved: ${saved.memoryPath}`);
    console.log(`Graph node: ${saved.nodeId}`);
    if (!opts.outcome) {
      console.log(
        'note: no outcome recorded — pass --outcome helpful (or unhelpful); entries without an outcome are skipped by reflect'
      );
    }
    console.log('Run `astria update .` to re-embed and re-cluster.');
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}

export async function reflectCommand(opts: { graph: string }) {
  try {
    const lessons = reflectGraph(opts.graph);
    console.log(lessons);
  } catch (e: any) {
    console.error(`Error: ${e.message || e}`);
    process.exitCode = 1;
  }
}
