// Emits open-benchmarks-report.md from the result JSONs in bench-work/open.
// A missing results file renders as "pending" so the report can be
// regenerated as runs land.
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import path from 'node:path';

const work = path.resolve(import.meta.dirname, '..', '..', '..', 'bench-work', 'open');
const read = (f) => (existsSync(path.join(work, f)) ? JSON.parse(readFileSync(path.join(work, f), 'utf8')).summary : null);

const swebench = read('swebench-results-django.json');
const repoqaBase = read('repoqa-results-python.json');
const repoqaEmbed = read('repoqa-results-python-embed.json');
const hotpot = read('hotpotqa-results.json');
const locomo = read('locomo-qa-results.json');

const pct = (v) => (v == null ? '—' : `${(v * 100).toFixed(1)}%`);
const lines = [];
lines.push(`# Open benchmarks — first runs`);
lines.push('');
lines.push(`astria measured on external, published datasets with the protocols other systems report on. All runs below are **smoke subsets** — they validate the adapters and give first point estimates; none is a benchmark claim. Protocols, licenses, and full-run commands: [scripts/bench/open/README.md](../../scripts/bench/open/README.md). Structural graph, no LLM, unless a leg says otherwise.`);
lines.push('');
lines.push(`## Results`);
lines.push('');
if (swebench && swebench.configs) {
  lines.push(`### SWE-bench Verified localization (django smoke, n=${swebench.n})`);
  lines.push('');
  lines.push(`One graph per instance (base_commit checkout), four query configurations graded against developer-patch files (test files excluded) and gold functions from patch hunks:`);
  lines.push('');
  lines.push(`| configuration | file hit@1 | hit@3 | hit@5 | file MRR | func hit@1 | func hit@5 |`);
  lines.push(`|---|---|---|---|---|---|---|`);
  for (const [name, c] of Object.entries(swebench.configs)) {
    lines.push(`| ${name} | ${pct(c.file_hit1)} | ${pct(c.file_hit3)} | ${pct(c.file_hit5)} | ${c.file_mrr} | ${pct(c.func_hit1)} | ${pct(c.func_hit5)} |`);
  }
  lines.push('');
} else {
  lines.push(`### SWE-bench Verified localization — pending`);
  lines.push('');
}
if (repoqaBase) {
  lines.push(`### RepoQA find (retrieval variant)`);
  lines.push('');
  if (repoqaEmbed) {
    const common = 'paired on the needles both arms completed';
    lines.push(`Baseline vs embedding arm (EMBED=1 builds with --embed; ${common}):`);
    lines.push('');
    lines.push(`| arm | n | file hit@1 | file hit@5 | file MRR | func hit@1 | func hit@5 |`);
    lines.push(`|---|---|---|---|---|---|---|`);
    lines.push(`| baseline | ${repoqaBase.n} | ${pct(repoqaBase.file_hit1)} | ${pct(repoqaBase.file_hit5)} | ${repoqaBase.mrr_file} | ${pct(repoqaBase.func_hit1)} | ${pct(repoqaBase.func_hit5)} |`);
    lines.push(`| embed | ${repoqaEmbed.n}${repoqaEmbed.complete ? '' : ' (incomplete)'} | ${pct(repoqaEmbed.file_hit1)} | ${pct(repoqaEmbed.file_hit5)} | ${repoqaEmbed.mrr_file} | ${pct(repoqaEmbed.func_hit1)} | ${pct(repoqaEmbed.func_hit5)} |`);
  } else {
    lines.push(`baseline n=${repoqaBase.n}: file hit@1 ${pct(repoqaBase.file_hit1)}, hit@5 ${pct(repoqaBase.file_hit5)}, func hit@1 ${pct(repoqaBase.func_hit1)} (embed arm pending)`);
  }
  lines.push('');
}
if (hotpot) {
  lines.push(`### HotpotQA distractor retrieval (n=${hotpot.single.n})`);
  lines.push('');
  lines.push(`| variant | hit@1 | hit@5 | MRR | both-gold-in-5 |`);
  lines.push(`|---|---|---|---|---|`);
  for (const [k, label] of [['single', 'single'], ['twostage', 'two-stage (naive)'], ['merged', 'merged (stage-1 + stage-2 discoveries)']]) {
    const v = hotpot[k];
    lines.push(`| ${label} | ${pct(v.hit1)} | ${pct(v.hit5)} | ${v.mrr} | ${pct(v.both_gold_in_5)} |`);
  }
  lines.push('');
}
if (locomo) {
  lines.push(`### LOCOMO QA accuracy (Graphify-protocol shape, n=${locomo.n})`);
  lines.push('');
  lines.push(`accuracy @coverage≥0.5 **${pct(locomo.qa_accuracy_at_least_half_coverage)}**, mean coverage **${locomo.mean_coverage}** — gpt-4o-mini reader+judge; directional only vs Graphify's 45.3% (n=300, shared Kimi K2.6).`);
  lines.push('');
}
lines.push(`## Notes`);
lines.push('');
if (swebench) {
  if (swebench && swebench.configs) {
  lines.push(`- **SWE-bench**: cleaned issue text (traceback/file-line stripping) is compared against raw at identical budget; the sweep's best configuration by file MRR is \`${swebench.best}\`.`);
}
}
if (hotpot) {
  lines.push(`- **HotpotQA**: naive two-stage seeding is a recorded negative result (biases toward the first hop); the merged variant keeps stage-1 quality and adds second-hop gold documents.`);
}
if (repoqaEmbed && !repoqaEmbed.complete) {
  const fails = (repoqaEmbed.failed_repos || []).map(f => f.repo).join(', ');
  lines.push(`- **RepoQA embed arm incomplete**: ${fails} failed${fails ? ' (embed char-boundary panic on multi-byte content — fixed in astria-embed after this run)' : ''}; paired deltas above use the common needles only.`);
}
lines.push(`- **LOCOMO QA**: cross-publishable numbers require running both systems under one shared model.`);
lines.push('');

writeFileSync(path.join(work, 'open-benchmarks-report.md'), lines.join('\n') + '\n');
console.log(lines.join('\n'));
