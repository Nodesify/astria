// hook-guard: editor PreToolUse guards (port of upstream hook-guard). Reads
// the tool-call JSON from stdin, nudges agents toward graph queries, and —
// in strict mode — denies ONE un-indexed read per session until the agent
// orients via query/explain/path. Fails open on any error: a wedged guard
// must never break a tool call.

import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'fs';
import { join, resolve, extname } from 'path';

interface ToolInput {
  tool_name?: string;
  tool_input?: { command?: string; pattern?: string; file_path?: string };
}

const SOURCE_EXTS = new Set([
  '.py', '.ts', '.tsx', '.js', '.jsx', '.rs', '.go', '.java', '.c', '.cpp',
  '.h', '.hpp', '.rb', '.php', '.swift', '.kt', '.scala', '.cs', '.lua',
  '.ex', '.exs', '.hs', '.sh', '.dart', '.zig', '.md', '.toml', '.yaml', '.yml',
]);

const SEARCH_COMMANDS = new Set(['grep', 'rg', 'ag', 'git']);

function readStdinJson(): ToolInput | null {
  try {
    const raw = readFileSync(0, 'utf-8');
    return JSON.parse(raw) as ToolInput;
  } catch {
    return null;
  }
}

function graphDir(): string | null {
  const cwd = process.env.CLAUDE_PROJECT_DIR || process.cwd();
  const g = join(cwd, '.graphify');
  return existsSync(join(g, 'graph.json')) ? g : null;
}

function orientedRecently(gdir: string): boolean {
  try {
    const stamp = readFileSync(join(gdir, 'cache', 'last_query_stamp'), 'utf-8');
    const ts = Number(stamp.split('\t')[0]);
    const age = Date.now() / 1000 - ts;
    const ttl = Number(process.env.GRAPHIFY_HOOK_STRICT_TTL || 1800);
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
      process.env.GRAPHIFY_HOOK_STRICT === '1' ||
      (process.env.GRAPHIFY_HOOK_STRICT !== '0' && _args.includes('--strict'));

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
        emitNudge(
          'MANDATORY: Before searching with grep, check the knowledge graph first: ' +
            '`nodesify-graphify query "<question>"` returns structured answers with ' +
            'file:line provenance in one call. The graph is already built for this repo.'
        );
      }
      return;
    }

    if (mode === 'read') {
      const filePath = input.tool_input?.file_path;
      if (!filePath) return;
      const resolved = resolve(filePath);
      if (resolved.includes('.graphify')) return;
      const ext = extname(resolved).toLowerCase();
      if (ext && !SOURCE_EXTS.has(ext)) return;
      // Staleness: file newer than the graph?
      try {
        const fileMtime = require('fs').statSync(resolved).mtimeMs;
        const graphMtime = require('fs').statSync(join(gdir, 'db.sqlite')).mtimeMs;
        if (fileMtime > graphMtime) {
          emitNudge(
            `STALE GRAPH: ${filePath} changed after the last graph build. ` +
              'Run `nodesify-graphify update .` before trusting graph answers.'
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
              'Run `nodesify-graphify query "<question>"` (or explain/path) once — ' +
              'further reads then proceed without interruption for 30 minutes.'
          );
        } catch {
          // Marker exists: already denied once this session — allow.
        }
      }
      return;
    }
  } catch {
    // Fail open: never break a tool call.
  }
}
