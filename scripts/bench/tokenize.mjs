// Shared exact tokenizer for benchmark parity: ONE tokenizer (OpenAI
// o200k_base via js-tiktoken) counts corpus and query tokens for BOTH astria
// and the original graphify, so absolute numbers are directly comparable —
// the "~87k vs ~158k" divergence came from each tool's own chars-per-token
// estimator, not from the files.
//
// Graceful degradation: when js-tiktoken is not installed the callers fall
// back to the heuristic estimate (bytes/4) and say so in their output.
//
// Usage:
//   import { loadTokenizer, countTokens, corpusTokensExact } from './tokenize.mjs';
//   const tok = await loadTokenizer();          // null when not installed
//   tok.count('some text')                       // exact o200k_base count
//   const { tokens, bytes, files, exact } = await corpusTokensExact(dir);

import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

const SKIP_DIRS = new Set([
  '.git', '.astria', '.graphify', '.github', 'node_modules', 'target',
  'bench-work', 'graphify-out', 'dist', 'coverage', '.venv', 'venv',
]);
// Dot-dirs that are still part of the corpus a naive agent would read.
const CORPUS_DOT_DIRS = new Set(['.github']);

const isSkipped = (name) =>
  SKIP_DIRS.has(name) ||
  (name.startsWith('.') && !CORPUS_DOT_DIRS.has(name));

const TEXT_EXTENSIONS = new Set([
  '.rs', '.py', '.ts', '.tsx', '.js', '.mjs', '.cjs', '.jsx', '.go', '.java',
  '.c', '.h', '.cpp', '.hpp', '.cc', '.rb', '.swift', '.kt', '.scala', '.php',
  '.cs', '.lua', '.hs', '.ex', '.exs', '.sh', '.bash', '.dart', '.zig', '.css',
  '.md', '.txt', '.json', '.jsonl', '.yaml', '.yml', '.toml', '.xml', '.html',
  '.sql', '.proto', '.graphql', '.dockerfile', '.env', '.cfg', '.ini', '.lock',
]);

export async function loadTokenizer() {
  try {
    const { encodingForModel } = await import('js-tiktoken');
    const enc = encodingForModel('gpt-4o'); // o200k_base
    return { name: 'o200k_base', implementation: 'js-tiktoken', count: (t) => enc.encode(t).length };
  } catch {
    return null;
  }
}

function collectFiles(root, out = []) {
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    if (isSkipped(entry.name)) continue;
    const p = path.join(root, entry.name);
    if (entry.isDirectory()) collectFiles(p, out);
    else if (TEXT_EXTENSIONS.has(path.extname(entry.name).toLowerCase())) out.push(p);
  }
  return out;
}

/// Exact o200k_base token count over every text file under `root` — the
/// corpus a naive agent would read in full. Returns null counts when no
/// tokenizer is available (caller must fall back to the heuristic).
export async function corpusTokensExact(root, tok) {
  const files = collectFiles(root);
  let bytes = 0;
  let tokens = 0;
  for (const f of files) {
    const buf = readFileSync(f);
    bytes += buf.length;
    if (tok) tokens += tok.count(buf.toString('utf8'));
  }
  return { tokens: tok ? tokens : null, bytes, files: files.length, exact: Boolean(tok) };
}

/// The query side of the parity block: exact tokens for one answer text,
/// with the heuristic fallback made explicit.
export function countTokens(text, tok) {
  return tok ? tok.count(text) : Math.ceil(Buffer.byteLength(text, 'utf8') / 4);
}
