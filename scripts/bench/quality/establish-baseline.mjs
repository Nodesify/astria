// Explicit reference publication; evaluation/comparison never updates a reference.
import { readFileSync, writeFileSync } from 'node:fs';
import { gate } from './gate.mjs';
const [input, output] = process.argv.slice(2);
if (!input || !output) throw Error('Usage: node establish-baseline.mjs measured-results.json NEW-baseline.json');
const payload = JSON.parse(readFileSync(input, 'utf8'));
if (!payload.comparison_identity || payload.reserved_consumed) throw Error('Require a current non-reserved measured result');
const failures = gate(payload);
if (failures.length) throw Error('Cannot establish failed reference: ' + failures.join('; '));
writeFileSync(output, JSON.stringify(payload, null, 2) + '\n', { flag: 'wx' });
