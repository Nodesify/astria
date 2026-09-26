#!/usr/bin/env node
// Prepare the LoCoMo long-conversation memory benchmark for astria:
// download the dataset, write each session as a transcript sidecar under
// <corpus>/.astria/transcripts/, and emit a QA file whose evidence entries
// are session files, so retrieval can be scored.
//
//   node scripts/bench/memory/prepare-locomo.mjs \
//     [--input <locomo10.json path|URL>] \
//     [--corpus bench-work/locomo-corpus] [--qa bench-work/locomo-qa.jsonl]
//
// Dataset shape (snap-research/locomo, ACL 2024): an array of conversations;
// each has `conversation` = { session_N: [{speaker, dia_id: "D1:3", text}],
// session_N_date_time, ... } and `qa` = [{question, answer,
// evidence: ["D1:3"], category}]. CC BY-NC 4.0 — research use.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '..', '..', '..');
const DEFAULT_URL = 'https://raw.githubusercontent.com/snap-research/locomo/main/data/locomo10.json';

function parseArgs(argv) {
  const o = {
    input: DEFAULT_URL,
    corpus: path.join(repoRoot, 'bench-work', 'locomo-corpus'),
    qa: path.join(repoRoot, 'bench-work', 'locomo-qa.jsonl'),
  };
  for (let i = 2; i < argv.length; i++) {
    if (argv[i] === '--input') o.input = argv[++i];
    else if (argv[i] === '--corpus') o.corpus = path.resolve(argv[++i]);
    else if (argv[i] === '--qa') o.qa = path.resolve(argv[++i]);
    else { console.error(`unknown arg: ${argv[i]}`); process.exit(2); }
  }
  return o;
}

async function fetchDataset(input) {
  if (!/^https?:/.test(input)) return JSON.parse(readFileSync(input, 'utf8'));
  const cache = input === DEFAULT_URL
    ? path.join(repoRoot, 'bench-work', 'locomo10.json')
    : null;
  if (cache && existsSync(cache)) return JSON.parse(readFileSync(cache, 'utf8'));
  console.log(`downloading ${input} ...`);
  const res = await fetch(input);
  if (!res.ok) throw new Error(`download failed: ${res.status}`);
  const text = await res.text();
  if (cache) { mkdirSync(path.dirname(cache), { recursive: true }); writeFileSync(cache, text); }
  return JSON.parse(text);
}

function prepareConversation(conv) {
  // sample_id looks like "conv-26" — the digits are the conversation id.
  const cid = Number(String(conv.sample_id ?? '').replace(/\D/g, '')) || 0;
  const convObj = conv.conversation ?? {};
  const sessionNums = Object.keys(convObj)
    .filter((k) => /^session_\d+$/.test(k))
    .map((k) => Number(k.slice('session_'.length)))
    .sort((a, b) => a - b);
  if (sessionNums.length === 0) return { cid, sessions: 0, qa: [] };

  const cid2 = String(cid).padStart(2, '0');
  const turnToSession = new Map(); // dia_id -> transcript file name
  let sessions = 0;
  for (const n of sessionNums) {
    const turns = convObj[`session_${n}`] ?? [];
    if (!Array.isArray(turns) || turns.length === 0) continue;
    const name = `conv${cid2}-session${String(n).padStart(2, '0')}.md`;
    const when = convObj[`session_${n}_date_time`];
    const body = [
      `# Conversation ${cid} — Session ${n}`,
      when ? `_Date: ${when}_` : '',
      '',
      ...turns.map((t) => `[${t.dia_id ?? ''}] **${t.speaker}:** ${t.text ?? ''}`),
      '',
    ].filter((l) => l !== '').join('\n') + '\n';
    writeFileSync(path.join(transcriptsDir, name), body);
    sessions++;
    for (const t of turns) if (t.dia_id) turnToSession.set(t.dia_id, name);
  }

  const qa = (conv.qa ?? []).map((row, i) => ({
    id: `c${cid}-q${i}`,
    conversation_id: cid,
    question: row.question,
    answer: String(row.answer ?? ''),
    category: row.category,
    evidence_files: [...new Set((row.evidence ?? [])
      .map((e) => turnToSession.get(e))
      .filter(Boolean))],
  }));
  return { cid, sessions, qa };
}

let transcriptsDir; // set in main()

async function main() {
  const o = parseArgs(process.argv);
  const data = await fetchDataset(o.input);
  transcriptsDir = path.join(o.corpus, '.astria', 'transcripts');
  mkdirSync(transcriptsDir, { recursive: true });

  const qaRows = [];
  let totalSessions = 0;
  for (const conv of data) {
    const { sessions, qa } = prepareConversation(conv);
    totalSessions += sessions;
    qaRows.push(...qa);
  }

  writeFileSync(o.qa, qaRows.map((r) => JSON.stringify(r)).join('\n') + '\n');
  const withEvidence = qaRows.filter((r) => r.evidence_files.length > 0).length;
  console.log(
    `locomo ready: ${totalSessions} session transcripts -> ${transcriptsDir}\n` +
    `${qaRows.length} QA pairs (${withEvidence} with resolvable evidence) -> ${o.qa}\n` +
    `build the graph: astria run ${o.corpus}\n` +
    `score retrieval: node scripts/bench/memory/run-locomo.mjs --corpus ${o.corpus} --qa ${o.qa}`,
  );
}

main().catch((e) => { console.error(e.message); process.exit(1); });
