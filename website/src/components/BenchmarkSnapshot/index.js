import React from 'react';

import snapshot from '../../data/benchmarks-snapshot.json';

function fmtSeconds(s) {
  return s == null ? '—' : `${s}s`;
}

function MetricRow({ label, a, b }) {
  return (
    <tr>
      <td>{label}</td>
      <td>{a}</td>
      <td>{b}</td>
    </tr>
  );
}

export default function BenchmarkSnapshot() {
  const orig = snapshot.original_tool;
  const ours = snapshot.nodesify_graphify_structural;
  return (
    <div>
      <p>
        <strong>Latest snapshot</strong> — {snapshot.runner}, generated{' '}
        {new Date(snapshot.generated_at).toISOString().slice(0, 10)} · corpus:{' '}
        <a href={`${snapshot.corpus.repo}/tree/${snapshot.corpus.commit}`}>
          graphify @ {snapshot.corpus.commit}
        </a>{' '}
        ({snapshot.corpus.files_detected} detected entries) · ours{' '}
        {snapshot.versions.nodesify_graphify} vs original ({snapshot.versions.python})
      </p>
      <table>
        <thead>
          <tr>
            <th>Metric</th>
            <th>original graphify</th>
            <th>nodesify-graphify</th>
          </tr>
        </thead>
        <tbody>
          <MetricRow label="Build time" a={fmtSeconds(orig.build_seconds)} b={fmtSeconds(ours.build_seconds)} />
          <MetricRow label="Nodes" a={orig.nodes} b={ours.nodes} />
          <MetricRow label="Edges" a={orig.edges} b={ours.edges} />
          <MetricRow label="Communities" a={orig.communities} b={ours.communities} />
          <MetricRow
            label="Token benchmark (own methodology)"
            a={orig.benchmark.reduction ? `${orig.benchmark.reduction}×` : '—'}
            b={ours.benchmark.reduction ? `${ours.benchmark.reduction}×` : '—'}
          />
          <MetricRow
            label="Avg query cost"
            a={orig.benchmark.avg_query_tokens ? `~${orig.benchmark.avg_query_tokens} tok` : '—'}
            b={ours.benchmark.avg_query_tokens ? `~${ours.benchmark.avg_query_tokens} tok` : '—'}
          />
        </tbody>
      </table>
      <ul>
        {snapshot.methodology_notes.map((note) => (
          <li key={note}>{note}</li>
        ))}
      </ul>
      <p>
        Re-run it any time: <strong>Actions → Benchmark snapshot → Run workflow</strong>. The
        workflow runs both tools on a fresh runner, commits the updated JSON, and the site
        redeploys automatically.
      </p>
    </div>
  );
}
