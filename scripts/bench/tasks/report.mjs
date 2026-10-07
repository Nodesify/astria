import path from 'node:path';
import { assert, evidence, hashFile, readJson, sha } from './common.mjs';

const [input, reviewFile] = process.argv.slice(2);
assert(input, 'Usage: node scripts/bench/tasks/report.mjs results.json [reviews.json]');
const results = readJson(input);
assert(results.schema_version === 1 && Array.isArray(results.runs), 'Unsupported results schema');
const reviews = reviewFile ? readJson(reviewFile) : null;
if (reviews) assert(reviews.results_sha256 === hashFile(input) && Array.isArray(reviews.entries), 'Review must pin exact immutable results.json');
const seen = new Set();
const reviewEvidence = [];
for (const entry of reviews?.entries ?? []) {
  const key = `${entry.task_id}:${entry.condition}`;
  assert(!seen.has(key), 'Duplicate review entry'); seen.add(key);
  const task = results.tasks.find(task => task.id === entry.task_id);
  assert(task && ['baseline', 'astria'].includes(entry.condition) && entry.rubric_sha256 === task.rubric_sha256, 'Review must match a task condition and rubric hash');
  assert(typeof entry.reviewer === 'string' && entry.reviewer.length > 0 && ['correct', 'incorrect', 'unknown'].includes(entry.decision), 'Provide explicit reviewer and decision');
  const proof = evidence(path.dirname(path.resolve(reviewFile)), entry.evidence);
  assert(proof, 'Correctness reviews require retained evidence');
  reviewEvidence.push({ task_id: entry.task_id, condition: entry.condition, files: proof });
  assert(Array.isArray(entry.criteria) && entry.criteria.length > 0 && entry.criteria.every(c => typeof c.id === 'string' && ['pass', 'fail', 'unknown'].includes(c.status)), 'Provide criterion-by-criterion review');
  assert(new Set(entry.criteria.map(c => c.id)).size === entry.criteria.length, 'Duplicate criterion IDs');
  assert(sha(JSON.stringify(task.rubric)) === task.rubric_sha256 && entry.criteria.length === task.rubric.length && task.rubric.every(c => entry.criteria.some(actual => actual.id === c.id)), 'Review every pinned rubric criterion exactly once');
  assert(entry.decision !== 'correct' || entry.criteria.every(c => c.status === 'pass'), 'Correct requires every rubric criterion to pass');
}
const reviewed = results.runs.map(row => {
  const review = reviews?.entries.find(entry => entry.task_id === row.task_id && entry.condition === row.condition);
  return { ...row, correctness: review?.decision ?? 'unknown' };
});
function verifyMetric(root, metric) {
  if (!Array.isArray(metric?.evidence)) return;
  const actual = evidence(root, metric.evidence.map(item => item.path));
  assert(actual?.every((item, index) => item.sha256 === metric.evidence[index].sha256), 'Recorded measurement evidence changed');
}
for (const row of reviewed) {
  const runRoot = path.join(path.dirname(path.resolve(input)), `${row.task_id}-${row.condition}`);
  verifyMetric(runRoot, row.tokens); verifyMetric(runRoot, row.source_reads);
}
for (const task of results.tasks) {
  const indexing = task.projects.astria.indexing;
  assert(hashFile(indexing.artifact_path) === indexing.artifact_sha256, 'Indexing provenance artifact changed');
  verifyMetric(path.dirname(indexing.artifact_path), indexing.initial_build_seconds);
  verifyMetric(path.dirname(indexing.artifact_path), indexing.update_seconds);
}
const finite = metric => typeof metric?.value === 'number' && Number.isFinite(metric.value) && metric.value >= 0 && metric.evidence;
const comparisons = results.tasks.map(task => {
  const baseline = reviewed.find(row => row.task_id === task.id && row.condition === 'baseline');
  const astria = reviewed.find(row => row.task_id === task.id && row.condition === 'astria');
  const comparable = [baseline, astria].every(row => row?.status === 'completed' && row.model_verified === true && row.runtime_verified === true && row.post_commit === task.commit && row.correctness === 'correct');
  const savings = metric => comparable && finite(baseline[metric]) && finite(astria[metric]) ? baseline[metric].value - astria[metric].value : null;
  const elapsedSaved = savings('elapsed_seconds');
  const initial = task.projects.astria.indexing.initial_build_seconds;
  const updates = task.projects.astria.indexing.update_seconds;
  const indexingSeconds = finite(initial) && finite(updates) ? initial.value + updates.value : null;
  return { task_id: task.id, comparable_correct_pair: comparable,
    baseline_status: baseline?.status ?? 'missing', astria_status: astria?.status ?? 'missing',
    baseline_correctness: baseline?.correctness ?? 'unknown', astria_correctness: astria?.correctness ?? 'unknown',
    elapsed_seconds_saved: elapsedSaved, tokens_saved: savings('tokens'), source_reads_saved: savings('source_reads'),
    wrong_file_edits_saved: savings('wrong_file_edit_count'), indexing_seconds: indexingSeconds,
    cold_task_seconds_saved_after_indexing: comparable && elapsedSaved !== null && indexingSeconds !== null ? elapsedSaved - indexingSeconds : null,
    reuse_break_even_tasks: comparable && elapsedSaved > 0 && indexingSeconds !== null ? Math.ceil(indexingSeconds / elapsedSaved) : null };
});
const conditions = Object.fromEntries(['baseline', 'astria'].map(condition => {
  const rows = reviewed.filter(row => row.condition === condition);
  return [condition, { planned: results.tasks.length, attempted: rows.filter(row => row.status !== 'not-started').length,
    completed: rows.filter(row => row.status === 'completed').length,
    correct: rows.filter(row => row.status === 'completed' && row.correctness === 'correct').length,
    incorrect: rows.filter(row => row.correctness === 'incorrect').length,
    unknown_correctness: rows.filter(row => row.correctness === 'unknown').length,
    failed_or_unfinished: rows.filter(row => row.status !== 'completed' && row.status !== 'not-started').length,
    success_rate_over_all_planned: rows.filter(row => row.status === 'completed' && row.correctness === 'correct').length / results.tasks.length }];
}));
const metrics = ['elapsed_seconds_saved', 'tokens_saved', 'source_reads_saved', 'wrong_file_edits_saved'];
const paired = Object.fromEntries(metrics.map(metric => {
  const values = comparisons.map(row => row[metric]).filter(value => value !== null);
  return [metric, { comparable_measured_pairs: values.length, mean_saved: values.length ? values.reduce((a, b) => a + b, 0) / values.length : null }];
}));
console.log(JSON.stringify({ schema_version: 1, results_sha256: hashFile(input), reviews_sha256: reviewFile ? hashFile(reviewFile) : null,
  review_evidence: reviewEvidence, agent_settings_sha256: sha(JSON.stringify(results.agent)), conditions, paired, comparisons,
  limits: [
    'No unreviewed, failed, or noncomparable run supports an improvement claim.',
    'Positive savings favor Astria; negative savings favor baseline. Unknown measurements remain null.',
    'Break-even assumes repeated comparable tasks with the measured mean behavior of that task and reuse of its graph; future update costs are not predicted.',
    'Run order alternates across tasks. No filesystem-cache flushing or statistical significance is implied.',
    'Adapter attestations and instrumentation must be audited; the harness does not independently observe remote model choice or reads.',
  ] }, null, 2));
