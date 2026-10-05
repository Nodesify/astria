// Question-only deterministic search. Expectations are deliberately not accepted.
import { spawnSync } from 'node:child_process';
import { openSync, closeSync, readSync, statSync, existsSync } from 'node:fs';
import path from 'node:path';

const norm = p => p.replaceAll('\\', '/').replace(/^\.\//, '');
const STOP = new Set('the how does where what and for are with which this that from into when why can its through before after'.split(' '));
const CONTROL = new Set(['if', 'for', 'while', 'switch', 'catch', 'with']);
const comparePaths = (a, b) => a < b ? -1 : a > b ? 1 : 0;
const excluded = file => /(?:^|\/)(?:\.git|\.astria|\.graphify|node_modules|target|dist|bench-work|golden)(?:\/|$)/.test(file) || file.startsWith('scripts/bench/') || file.endsWith('.jsonl');
const terms = question => [...new Set(question.match(/[A-Za-z_][A-Za-z_0-9]{2,}/g) || [])].filter(w => !STOP.has(w.toLowerCase())).slice(0, 20);
const declarations = /(?:\b(?:async\s+)?(?:function|def|fn|class|struct|enum|interface|type)\s+([A-Za-z_$][\w$]*)|\b(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s*)?(?:function|\([^)]*\)\s*=>)|^\s*(?:async\s+)?([A-Za-z_$][\w$]*)\s*\([^;]*\)\s*\{)/;

export function lexicalBaseline({ root, question, tok, budget, iterative = false }) {
  const costs = { rounds: [], search_seconds: 0, search_output_bytes: 0, search_output_tokens: 0, read_seconds: 0, read_bytes: 0, read_tokens: 0, read_files: 0, failures: [] };
  const ranked = new Map(), read = new Set(), windows = new Map();
  let searchTerms = terms(question), context = '';
  for (let round = 0; round < (iterative ? 3 : 1); round++) {
    if (!searchTerms.length) break;
    const started = performance.now();
    const search = spawnSync('rg', ['--json', '--sort', 'path', '-i', '-F', '-m', '3', '-g', '!.astria/**', '-g', '!.graphify/**', '-g', '!node_modules/**', '-g', '!target/**', '-g', '!dist/**', '-g', '!bench-work/**', '-g', '!scripts/bench/**', '-g', '!**/golden/**', '-g', '!**/*.jsonl', ...searchTerms.flatMap(w => ['-e', w]), '--', '.'], { cwd: root, encoding: 'utf8', timeout: 30000, maxBuffer: 16 * 1024 * 1024 });
    const searchSeconds = (performance.now() - started) / 1000;
    costs.search_seconds += searchSeconds;
    costs.search_output_bytes += Buffer.byteLength(search.stdout || '');
    costs.search_output_tokens += tok.count(search.stdout || '');
    const trace = { round: round + 1, terms: searchTerms, search_seconds: searchSeconds, matches_considered: 0, files_read: [], read_windows: [], refinement: [] };
    costs.rounds.push(trace);
    if (search.error || ![0, 1].includes(search.status)) {
      costs.failures.push(`round ${round + 1}: ${search.error?.message || search.stderr || `rg exit ${search.status}`}`);
      break;
    }
    for (const record of (search.stdout || '').split('\n')) {
      if (!record || trace.matches_considered >= 2000) continue;
      let data; try { const row = JSON.parse(record); if (row.type !== 'match') continue; data = row.data; } catch { costs.failures.push(`round ${round + 1}: invalid rg JSON`); continue; }
      if (!data.path.text || !data.lines.text) continue;
      trace.matches_considered++;
      const file = norm(data.path.text), old = ranked.get(file) || { file, line: data.line_number, score: 0 };
      old.score += searchTerms.filter(w => data.lines.text.toLowerCase().includes(w.toLowerCase())).length;
      ranked.set(file, old);
    }
    const evidence = new Set(), imports = new Set();
    const candidates = [...ranked.values()].sort((a, b) => b.score - a.score || comparePaths(a.file, b.file));
    for (const candidate of candidates.filter(c => !read.has(c.file)).slice(0, iterative ? 6 : 24)) {
      const absolute = path.resolve(root, candidate.file), relative = path.relative(root, absolute);
      if (relative.startsWith('..') || path.isAbsolute(relative) || excluded(norm(relative))) continue;
      const readStarted = performance.now();
      try {
        const info = statSync(absolute);
        if (!info.isFile()) continue;
        const fd = openSync(absolute, 'r'), buffer = Buffer.alloc(Math.min(info.size, 256 * 1024));
        let bytes; try { bytes = readSync(fd, buffer, 0, buffer.length, 0); } finally { closeSync(fd); }
        const source = buffer.subarray(0, bytes).toString('utf8');
        costs.read_bytes += bytes; costs.read_tokens += tok.count(source); costs.read_files++;
        if (info.size > bytes) costs.failures.push(`${candidate.file}: read capped at 256 KiB`);
        read.add(candidate.file); trace.files_read.push(candidate.file);
        const lines = source.split('\n'), start = Math.max(0, candidate.line - 11), end = Math.min(lines.length, candidate.line + 30);
        const window = lines.slice(start, end);
        let text = `FILE ${candidate.file}\n`;
        for (const [offset, line] of window.entries()) {
          const declaration = line.match(declarations);
          if (declaration && !CONTROL.has(declaration.slice(1).find(Boolean))) text += `DEFINITION ${candidate.file}:${start + offset + 1} ${declaration.slice(1).find(Boolean)}\n`;
          text += `L${start + offset + 1} ${line}\n`;
          for (const m of line.matchAll(/\b([A-Za-z_][\w]{2,})\s*\(/g)) if (!STOP.has(m[1].toLowerCase())) evidence.add(m[1]);
          for (const m of line.matchAll(/(?:from\s+|require\s*\(\s*|import\s*)['"]([^'"]+)['"]/g)) {
            if (m[1].startsWith('.')) imports.add(path.resolve(path.dirname(absolute), m[1]));
            else for (const name of m[1].match(/[A-Za-z_][\w]{2,}/g) || []) evidence.add(name);
          }
          const pyImport = line.match(/^\s*from\s+([\w.]+)\s+import\s+(.+)/);
          if (pyImport) for (const name of `${pyImport[1]} ${pyImport[2]}`.match(/[A-Za-z_][\w]{2,}/g) || []) evidence.add(name);
        }
        windows.set(candidate.file, text); trace.read_windows.push({ file: candidate.file, first_line: start + 1, last_line: end });
      } catch (error) { costs.failures.push(`${candidate.file}: ${error.message}`); }
      costs.read_seconds += (performance.now() - readStarted) / 1000;
      if (!iterative && tok.count([...windows.values()].join('')) >= budget) break;
    }
    for (const imported of [...imports].sort()) {
      for (const extension of ['', '.js', '.ts', '.py', '/index.js', '/index.ts']) {
        const target = imported + extension, relative = path.relative(root, target);
        if (!relative.startsWith('..') && !path.isAbsolute(relative) && !excluded(norm(relative)) && existsSync(target) && statSync(target).isFile()) {
          const file = norm(relative); if (!ranked.has(file)) ranked.set(file, { file, line: 1, score: 1 }); break;
        }
      }
    }
    searchTerms = [...evidence].sort().filter(w => !searchTerms.includes(w)).slice(0, 12);
    trace.refinement = searchTerms;
    if (!iterative || !searchTerms.length) break;
  }
  for (const candidate of [...ranked.values()].sort((a, b) => b.score - a.score || comparePaths(a.file, b.file))) {
    if (windows.has(candidate.file)) context += windows.get(candidate.file);
  }
  return { status: costs.failures.length && !context ? 1 : 0, stdout: context, stderr: costs.failures.join('\n'), costs };
}
