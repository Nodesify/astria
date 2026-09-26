#!/usr/bin/env node
// Generate promptfoo tests.generated.yaml from the golden QA set, so the
// blind judge and the deterministic harness always ask the same questions.
//
//   node scripts/bench/quality/promptfoo/gen-promptfoo-tests.mjs \
//     [golden.jsonl] [out.yaml]

import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const goldenPath = process.argv[2] || path.join(here, '..', 'golden', 'astria-self.jsonl');
const outPath = process.argv[3] || path.join(here, 'tests.generated.yaml');

const items = readFileSync(goldenPath, 'utf8')
  .split('\n').filter((l) => l.trim()).map((l) => JSON.parse(l));

const blocks = items.map((g) => {
  const rubric =
    `Ground truth: this is implemented in ${g.expected_files.join(', ')}` +
    (g.expected_symbols?.length ? ` (${g.expected_symbols.join(', ')})` : '') +
    `. Question asked: "${g.question}". Judge whether the response identifies ` +
    `that implementation — naming the file/symbol, or describing exactly ` +
    `what it does such that a developer could navigate there. A response ` +
    `that is merely topically related, or points at documentation rather ` +
    `than the implementing code, is a FAIL.`;
  return [
    '- vars:',
    `    question: ${JSON.stringify(g.question)}`,
    `    rubric: ${JSON.stringify(rubric)}`,
    "  assert:",
    "    - type: llm-rubric",
    "      value: '{{rubric}}'",
  ].join('\n');
});

writeFileSync(outPath, blocks.join('\n') + '\n');
console.log(`wrote ${items.length} tests -> ${outPath}`);
