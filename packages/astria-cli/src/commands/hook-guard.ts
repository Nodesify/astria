// hook-guard: editor PreToolUse guards (port of upstream hook-guard). Reads
// the tool-call JSON from stdin, nudges agents toward graph queries, and —
// in strict mode — denies ONE un-indexed read per session until the agent
// orients via query/explain/path. When the graph offers concrete evidence
// (symbols matching the search pattern, or symbols defined in the file
// being read), the generic nudge is replaced with those symbols, their
// file:line locations, and a ready-to-run command. Fails open on any
// error: a wedged guard must never break a tool call.

import { existsSync, readFileSync, writeFileSync, mkdirSync, statSync } from 'fs';
import { join, resolve, extname } from 'path';
import { writeTextAtomic } from '../install/atomic';

interface ToolInput {
  tool_name?: string;
  tool_input?: { command?: string; pattern?: string; file_path?: string };
}

export interface SymbolEntry {
  /** Node id. */
  i: string;
  /** Display label, e.g. `hookGuard()`. */
  l: string;
  /** Source file as stored in the graph (absolute, forward slashes). */
  f: string;
  /** Source line. */
  n: number;
  /** Graph degree (edge endpoint count) — used to rank hubs first. */
  d: number;
}

const SOURCE_EXTS = new Set([
  '.py', '.ts', '.tsx', '.js', '.jsx', '.rs', '.go', '.java', '.c', '.cpp',
  '.h', '.hpp', '.rb', '.php', '.swift', '.kt', '.scala', '.cs', '.lua',
  '.ex', '.exs', '.hs', '.sh', '.dart', '.zig', '.md', '.toml', '.yaml', '.yml',
]);

const SEARCH_COMMANDS = new Set(['grep', 'rg', 'ag', 'git']);

const SEARCH_TOOLS = new Set(['grep', 'rg', 'ag']);

// Contextual-guidance bounds: at most this many symbols per nudge, each
// nudge repeated at most once per key within the TTL, and the underlying
// graph.json parse refused beyond this size (a 20 MB graph parses in
// ~120 ms once; the derived index then serves every later call).
const CONTEXT_SYMBOL_LIMIT = 5;
const CONTEXT_DEFAULT_TTL_SECONDS = 1800;
const CONTEXT_MAX_GRAPH_BYTES = 200 * 1024 * 1024;
// Index cap for pathological repos: keeping the highest-degree symbols
// bounds the per-tool-call index parse to a few milliseconds.
const CONTEXT_MAX_INDEX_SYMBOLS = 50_000;
const CONTEXT_MESSAGE_LIMIT = 1200;

function readStdinJson(): ToolInput | null {
  try {
    const raw = readFileSync(0, 'utf-8');
    return JSON.parse(raw) as ToolInput;
  } catch {
    return null;
  }
}

function projectRootDir(): string {
  return process.env.CLAUDE_PROJECT_DIR || process.cwd();
}

function graphDir(): string | null {
  const g = join(projectRootDir(), '.astria');
  return existsSync(join(g, 'graph.json')) ? g : null;
}

/// Reads `ASTRIA_<name>`, falling back to the deprecated `GRAPHIFY_<name>`.
function envVar(name: string): string | undefined {
  return (
    process.env[`ASTRIA_${name}`] ||
    process.env[`GRAPHIFY_${name}`] ||
    undefined
  );
}

function orientedRecently(gdir: string): boolean {
  try {
    const stamp = readFileSync(join(gdir, 'cache', 'last_query_stamp'), 'utf-8');
    const ts = Number(stamp.split('\t')[0]);
    const age = Date.now() / 1000 - ts;
    const ttl = Number(envVar('HOOK_STRICT_TTL') || 1800);
    return Number.isFinite(ts) && age < ttl;
  } catch {
    return false;
  }
}

function emitNudge(text: string): void {
  console.log(
    JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        additionalContext: text,
      },
    })
  );
}

function emitDeny(reason: string): void {
  console.log(
    JSON.stringify({
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'deny',
        permissionDecisionReason: reason,
      },
    })
  );
}

// ---- contextual guidance: symbol index ----

function indexCachePath(gdir: string): string {
  return join(gdir, 'cache', 'hook-index.json');
}

/// Compact symbol index derived from graph.json: only located code/test
/// symbols make useful guidance, and a ~250 KB cache file parses in a few
/// milliseconds where the full 20+ MB graph takes ~100+. Rebuilt whenever
/// the graph is newer than the index. Any failure yields null and callers
/// fall back to the generic nudge (fail-open).
export function symbolIndex(gdir: string): SymbolEntry[] | null {
  try {
    const graphPath = join(gdir, 'graph.json');
    const cachePath = indexCachePath(gdir);
    try {
      if (existsSync(cachePath) && statSync(cachePath).mtimeMs >= statSync(graphPath).mtimeMs) {
        const cached = JSON.parse(readFileSync(cachePath, 'utf8'));
        if (cached && Array.isArray(cached.symbols)) return cached.symbols as SymbolEntry[];
      }
    } catch {
      // Corrupt or half-written cache — fall through to a rebuild.
    }

    if (statSync(graphPath).size > CONTEXT_MAX_GRAPH_BYTES) return null;
    const raw = JSON.parse(readFileSync(graphPath, 'utf8'));
    const degree = new Map<string, number>();
    for (const e of raw.edges || []) {
      degree.set(e.source, (degree.get(e.source) || 0) + 1);
      degree.set(e.target, (degree.get(e.target) || 0) + 1);
    }
    const symbols: SymbolEntry[] = [];
    for (const node of raw.nodes || []) {
      if (!node.source_file || typeof node.source_line !== 'number') continue;
      if (node.file_type !== 'code' && node.file_type !== 'test') continue;
      const label = typeof node.label === 'string' ? node.label : '';
      // Anonymous closures and yaml/document chunks read as noise, and file
      // nodes (no '::' in the id) have no location their label doesn't show.
      if (!label || label.startsWith('{') || !String(node.id || '').includes('::')) continue;
      symbols.push({ i: node.id, l: label, f: node.source_file, n: node.source_line, d: degree.get(node.id) || 0 });
    }
    if (symbols.length > CONTEXT_MAX_INDEX_SYMBOLS) {
      symbols.sort((a, b) => b.d - a.d || (a.i < b.i ? -1 : 1));
      symbols.length = CONTEXT_MAX_INDEX_SYMBOLS;
    }
    try {
      mkdirSync(join(gdir, 'cache'), { recursive: true });
      writeTextAtomic(cachePath, JSON.stringify({ v: 1, symbols }));
    } catch {
      // The cache write is best-effort; the in-memory index still serves.
    }
    return symbols;
  } catch {
    return null;
  }
}

// ---- contextual guidance: once-per-key emission ----

function contextStatePath(gdir: string): string {
  return join(gdir, 'cache', 'hook-context-emitted.json');
}

/// One contextual nudge per key (search pattern or file) per TTL — the same
/// evidence repeated on every tool call is noise, not guidance.
export function contextAlreadyEmitted(gdir: string, key: string, ttlSeconds: number): boolean {
  try {
    const state = JSON.parse(readFileSync(contextStatePath(gdir), 'utf8'));
    const ts = Number(state[key]);
    return Number.isFinite(ts) && Date.now() / 1000 - ts < ttlSeconds;
  } catch {
    return false;
  }
}

export function markContextEmitted(gdir: string, key: string, ttlSeconds: number): void {
  try {
    let state: Record<string, number> = {};
    try {
      state = JSON.parse(readFileSync(contextStatePath(gdir), 'utf8'));
    } catch {
      // Fresh state file.
    }
    const cutoff = Date.now() / 1000 - ttlSeconds * 2;
    for (const k of Object.keys(state)) {
      if (!Number.isFinite(state[k]) || state[k] < cutoff) delete state[k];
    }
    state[key] = Date.now() / 1000;
    const keys = Object.keys(state);
    if (keys.length > 500) {
      for (const k of keys.slice(0, keys.length - 500)) delete state[k];
    }
    mkdirSync(join(gdir, 'cache'), { recursive: true });
    writeTextAtomic(contextStatePath(gdir), JSON.stringify(state));
  } catch {
    // Dedupe bookkeeping is best-effort; worst case the nudge repeats.
  }
}

// ---- contextual guidance: matching and formatting ----

function normalizeForMatch(p: string): string {
  return p.replace(/\\/g, '/').toLowerCase();
}

/// Best-effort pattern extraction from a search command line: grep/rg/ag
/// take the first non-option argument as the pattern, `git grep` the first
/// after the subcommand. Quoted patterns are rejoined. Returns null when
/// nothing recognizable remains — callers then keep the generic nudge.
export function extractSearchPattern(command: string): string | null {
  const tokens = command.trim().split(/\s+/).filter(Boolean);
  if (tokens[0] === 'git') {
    const grepIdx = tokens.indexOf('grep');
    if (grepIdx === -1) return null;
    tokens.splice(0, grepIdx + 1);
  } else if (SEARCH_TOOLS.has(tokens[0])) {
    tokens.shift();
  } else {
    return null;
  }
  const idx = tokens.findIndex((t) => !t.startsWith('-'));
  if (idx === -1) return null;
  let pattern = tokens[idx];
  const quote = pattern[0];
  if ((quote === '"' || quote === "'") && !pattern.endsWith(quote)) {
    const parts = [pattern];
    for (let j = idx + 1; j < tokens.length; j++) {
      parts.push(tokens[j]);
      if (tokens[j].endsWith(quote)) break;
    }
    pattern = parts.join(' ');
  }
  if (quote === '"' || quote === "'") pattern = pattern.slice(1, -1);
  return pattern || null;
}

/// Ranks symbols against a search pattern: exact label beats prefix beats
/// substring beats a shared identifier token; graph degree breaks ties so
/// the real `foo()` outranks a same-named test helper.
export function matchSymbols(index: SymbolEntry[], pattern: string, limit: number): SymbolEntry[] {
  const needle = pattern.toLowerCase();
  const tokens = needle.match(/[a-z_][a-z0-9_]{2,}/g) || [];
  const scored: Array<{ e: SymbolEntry; s: number }> = [];
  for (const e of index) {
    const l = e.l.toLowerCase();
    let s = 0;
    if (l === needle) s = 4;
    else if (l.startsWith(needle)) s = 3;
    else if (l.includes(needle)) s = 2;
    else {
      for (const t of tokens) {
        if (l.includes(t)) {
          s = 1;
          break;
        }
      }
    }
    if (s > 0) scored.push({ e, s });
  }
  scored.sort((a, b) =>
    b.s - a.s || b.e.d - a.e.d || a.e.l.length - b.e.l.length || (a.e.i < b.e.i ? -1 : 1)
  );
  return scored.slice(0, limit).map((x) => x.e);
}

/// Symbols defined in the file being read. Paths are compared normalized
/// (the graph stores absolute forward-slash paths; reads arrive in the
/// host's native shape), with a defensive relative-path branch for graphs
/// built before absolute loci.
export function symbolsInFile(index: SymbolEntry[], root: string, absPath: string): SymbolEntry[] {
  const target = normalizeForMatch(absPath);
  const rootNorm = normalizeForMatch(root);
  const hits = index.filter((e) => {
    const f = normalizeForMatch(e.f);
    if (f === target) return true;
    if (!f.startsWith('/') && !/^[a-z]:/.test(f)) {
      return rootNorm && normalizeForMatch(join(root, e.f)) === target;
    }
    return false;
  });
  hits.sort((a, b) => b.d - a.d || a.n - b.n || (a.i < b.i ? -1 : 1));
  return hits;
}

/// Repo-relative display path (falls back to the stored path when the
/// prefix does not match, e.g. for graphs built elsewhere).
export function displayPath(root: string, f: string): string {
  const fn = normalizeForMatch(f);
  const rootNorm = normalizeForMatch(root);
  if (rootNorm && fn.startsWith(rootNorm + '/')) return f.slice(rootNorm.length + 1);
  return f;
}

function truncateLabel(label: string): string {
  return label.length > 48 ? label.slice(0, 45) + '…' : label;
}

/// Suggested commands are embedded in double quotes; strip characters that
/// would break out of them. The suggestion is advisory, not executed.
function suggestable(s: string): string {
  return s.replace(/["`$\\]/g, '');
}

/// `label (path:line)` entries for a search nudge; the list shrinks until
/// the whole message fits the bound.
export function formatSymbolList(entries: SymbolEntry[], root: string, limit: number): string {
  let parts = entries.map((e) => `${truncateLabel(e.l)} (${displayPath(root, e.f)}:${e.n})`);
  while (parts.length > 1 && parts.join(', ').length > limit) {
    parts = parts.slice(0, -1);
  }
  return parts.join(', ');
}

function contextTtlSeconds(): number {
  const ttl = Number(envVar('HOOK_CONTEXT_TTL'));
  return Number.isFinite(ttl) && ttl >= 0 ? ttl : CONTEXT_DEFAULT_TTL_SECONDS;
}

export function hookGuard(mode: string, _args: string[]): void {
  try {
    if (!mode || mode === 'gemini') {
      // Gemini's BeforeTool only understands allow decisions.
      console.log(JSON.stringify({ decision: 'allow' }));
      return;
    }

    const input = readStdinJson();
    if (!input) return;
    const gdir = graphDir();
    if (!gdir) return;

    const strict =
      envVar('HOOK_STRICT') === '1' ||
      (envVar('HOOK_STRICT') !== '0' && _args.includes('--strict'));
    const contextEnabled = envVar('HOOK_CONTEXT') !== '0';

    if (mode === 'search') {
      const ti = input.tool_input || {};
      const isGrepTool = Boolean(ti.pattern);
      const cmd = (ti.command || '').trim();
      const first = cmd.split(/\s+/)[0];
      const isSearchBash =
        first === 'git'
          ? cmd.split(/\s+/)[1] === 'grep'
          : SEARCH_COMMANDS.has(first);
      if (isGrepTool || isSearchBash) {
        const pattern = ((ti.pattern || '').trim() || extractSearchPattern(cmd) || '').trim();
        const index = contextEnabled && pattern ? symbolIndex(gdir) : null;
        const hits = index ? matchSymbols(index, pattern, CONTEXT_SYMBOL_LIMIT) : [];
        if (hits.length > 0) {
          const key = 'search:' + pattern.toLowerCase();
          const ttl = contextTtlSeconds();
          if (!contextAlreadyEmitted(gdir, key, ttl)) {
            markContextEmitted(gdir, key, ttl);
            const seed = suggestable(truncateLabel(hits[0].l));
            const needle = suggestable(pattern) || pattern;
            const list = formatSymbolList(hits, projectRootDir(), 700);
            emitNudge(
              `GRAPH-FIRST: ${hits.length === 1 ? 'a graph symbol already matches' : `${hits.length} graph symbols already match`} ` +
                `"${needle}": ${list}. ` +
                `Try \`astria query --seed "${seed}" "${needle}"\` or \`astria explain "${seed}"\` ` +
                'for structured answers with file:line provenance instead of grep.'
            );
          }
          return;
        }
        emitNudge(
          'MANDATORY: Before searching with grep, check the knowledge graph first: ' +
            '`astria query "<question>"` returns structured answers with ' +
            'file:line provenance in one call. The graph is already built for this repo.'
        );
      }
      return;
    }

    if (mode === 'read') {
      const filePath = input.tool_input?.file_path;
      if (!filePath) return;
      const resolved = resolve(filePath);
      if (resolved.includes('.astria')) return;
      const ext = extname(resolved).toLowerCase();
      if (ext && !SOURCE_EXTS.has(ext)) return;
      // Staleness: file newer than the graph?
      try {
        const fileMtime = statSync(resolved).mtimeMs;
        const graphMtime = statSync(join(gdir, 'db.sqlite')).mtimeMs;
        if (fileMtime > graphMtime) {
          emitNudge(
            `STALE GRAPH: ${filePath} changed after the last graph build. ` +
              'Run `astria update .` before trusting graph answers.'
          );
          return;
        }
      } catch {
        // stat failures are not guard failures
      }
      // Strict: deny one un-indexed read per session until oriented.
      if (strict && input.tool_name === 'Read' && !orientedRecently(gdir)) {
        // Session ids become filenames; strip anything that could carry a
        // path separator or traversal before using one.
        const session =
          (process.env.CLAUDE_SESSION_ID || 'default')
            .replace(/[^A-Za-z0-9_-]/g, '')
            .slice(0, 64) || 'default';
        const marker = join(gdir, 'cache', 'hook_sessions', `${session}.denied`);
        try {
          mkdirSync(join(gdir, 'cache', 'hook_sessions'), { recursive: true });
          writeFileSync(marker, String(Date.now()), { flag: 'wx' });
          emitDeny(
            'ORIENT FIRST: this read is not yet covered by a graph query. ' +
              'Run `astria query "<question>"` (or explain/path) once — ' +
              'further reads then proceed without interruption for 30 minutes.'
          );
        } catch {
          // Marker exists: already denied once this session — allow.
        }
        return;
      }
      // Contextual guidance: name the symbols the graph knows in this file
      // (hub-first) once per file per TTL.
      if (contextEnabled) {
        const index = symbolIndex(gdir);
        const hits = index ? symbolsInFile(index, projectRootDir(), resolved).slice(0, CONTEXT_SYMBOL_LIMIT) : [];
        if (hits.length > 0) {
          const key = 'read:' + normalizeForMatch(resolved);
          const ttl = contextTtlSeconds();
          if (!contextAlreadyEmitted(gdir, key, ttl)) {
            markContextEmitted(gdir, key, ttl);
            const hub = suggestable(truncateLabel(hits[0].l));
            const parts = hits.map((e) => `${truncateLabel(e.l)} (line ${e.n})`);
            while (parts.length > 1 && parts.join(', ').length > 700) {
              parts.length = parts.length - 1;
            }
            emitNudge(
              `GRAPH CONTEXT: the knowledge graph indexes ${hits.length === 1 ? 'a symbol' : `${hits.length} symbols`} in ` +
                `${displayPath(projectRootDir(), hits[0].f)}: ${parts.join(', ')}. ` +
                `Start with \`astria explain "${hub}"\` for their connections instead of reading line by line.`
            );
          }
        }
      }
      return;
    }
  } catch {
    // Fail open: never break a tool call.
  }
}
