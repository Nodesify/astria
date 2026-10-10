import { readFileSync } from 'node:fs';

export const contract = 'source-retrieval-v4';
export function gate(payload, { minRecall = 50, minDefinitionRecall = 50, baselineFile = null } = {}) {
  const failures = [];
  const s = payload.summary;
  if (s.failed || s.baseline_partial_failures) failures.push('query or lexical-search failures');
  if (s.delivered_budget_violations || (payload.method === 'astria' && s.raw_budget_violations)) failures.push('token budget exceeded');
  if (!s.definition_count) failures.push('no source-grounded definition expectations');
  for (const [metric, floor] of [['recall@5', minRecall], ['definition_recall@5', minDefinitionRecall]]) {
    if (!Number.isFinite(s[metric]) || s[metric] < floor / 100) failures.push(`${metric} below ${floor}%`);
  }
  if (baselineFile) {
    const baseline = JSON.parse(readFileSync(baselineFile, 'utf8'));
    if (JSON.stringify(payload.comparison_identity) !== JSON.stringify(baseline.comparison_identity)) {
      failures.push('baseline is incomparable: corpus, golden, harness, method, tokenizer, or query policy differs');
    } else {
      // A failed baseline is never an established quality reference.
      if (baseline.summary.failed || baseline.summary.baseline_partial_failures || baseline.summary.delivered_budget_violations || !baseline.summary.definition_count) failures.push('baseline contains failed measurements or has no grounded definitions');
      for (const metric of ['recall@5', 'mrr', 'definition_recall@5', 'definition_mrr']) {
        if (!Number.isFinite(baseline.summary[metric]) || s[metric] + 1e-12 < baseline.summary[metric]) failures.push(`${metric} regressed from established baseline`);
      }
    }
  }
  return failures;
}
