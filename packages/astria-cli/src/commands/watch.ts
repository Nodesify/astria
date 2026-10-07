import * as fs from 'fs';
import * as path from 'path';
import { spawn, type ChildProcess } from 'child_process';

// The watcher does not second-guess discovery: the pipeline's detect pass
// already classifies every supported input (code, docs, manifests, media,
// office, PDFs) and honors .gitignore/.astriaignore, so any event outside
// the known-irrelevant directories schedules a rebuild. Filtering to a
// small language subset here used to miss Ruby/PHP/Kotlin edits, Markdown
// and manifest changes, and directory renames entirely.
const SKIP_DIRS = new Set([
  '.astria',
  'node_modules',
  'target',
  '.git',
  'dist',
  '__pycache__',
  '.cache',
]);

export async function watchCommand(watchPath: string, opts: { debounce: string; maxWait?: string }) {
  const debounceMs = Number(opts.debounce || '3000');
  if (!Number.isFinite(debounceMs) || debounceMs <= 0) {
    console.error(`Error: invalid --debounce value "${opts.debounce}" (must be a positive number of milliseconds)`);
    process.exitCode = 1;
    return;
  }
  const maxWaitMs = Number(opts.maxWait ?? Math.max(debounceMs, Math.min(debounceMs * 5, 30_000)));
  if (!Number.isFinite(maxWaitMs) || maxWaitMs <= 0) {
    console.error('Error: --max-wait must be a positive number of milliseconds');
    process.exitCode = 1;
    return;
  }
  const resolved = path.resolve(watchPath);

  if (!fs.existsSync(resolved) || !fs.statSync(resolved).isDirectory()) {
    console.error(`Error: "${resolved}" is not a valid directory`);
    process.exitCode = 1;
    return;
  }

  let pendingEvents = new Set<string>();
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  // One rebuild at a time, at most one queued behind it: the rebuild runs
  // as a child process (the pipeline is synchronous native code — running
  // it inline would freeze the watcher for the whole build).
  let rebuildInFlight = false;
  let rebuildQueued = false;
  let activeChild: ChildProcess | null = null;
  let stopped = false;
  let maxWaitTimer: ReturnType<typeof setTimeout> | null = null;
  const clearTimers = () => {
    if (debounceTimer) clearTimeout(debounceTimer);
    if (maxWaitTimer) clearTimeout(maxWaitTimer);
    debounceTimer = maxWaitTimer = null;
  };

  const runRebuild = () => {
    clearTimers();
    if (stopped) return;
    if (rebuildInFlight) {
      rebuildQueued = true;
      return;
    }
    rebuildInFlight = true;
    console.log(`\n[astria] ${pendingEvents.size} change event(s), rebuilding...`);
    pendingEvents.clear();
    let finished = false;
    const complete = (code: number | null, error?: Error) => {
      if (finished) return;
      finished = true;
      activeChild = null;
      rebuildInFlight = false;
      if (error) console.error('[astria] Rebuild could not start:', error.message);
      else if (code !== 0 && !stopped) console.error(`[astria] Rebuild exited with code ${code}`);
      if (!stopped && rebuildQueued) {
        rebuildQueued = false;
        runRebuild();
      }
    };
    try {
      const child = spawn(process.execPath, [process.argv[1], 'update', resolved], { stdio: 'inherit' });
      activeChild = child;
      child.once('error', err => complete(null, err));
      child.once('exit', code => complete(code));
    } catch (err) {
      complete(null, err instanceof Error ? err : new Error(String(err)));
    }
  };

  const scheduleRebuild = () => {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(runRebuild, debounceMs);
    // Continuous edits cannot postpone reconciliation indefinitely.
    if (!maxWaitTimer) maxWaitTimer = setTimeout(runRebuild, maxWaitMs);
  };

  let watcher: fs.FSWatcher;
  try {
    watcher = fs.watch(resolved, { recursive: true }, (_event, filename) => {
      // A missing filename or an extension-less name is usually a
      // directory event (create/rename/delete): schedule a rebuild and
      // let discovery reconcile the tree.
      if (filename) {
        const filePath = filename.replace(/\\/g, '/');
        if (filePath.split('/').some((p: string) => SKIP_DIRS.has(p))) return;
        pendingEvents.add(filePath);
      } else {
        pendingEvents.add('(directory event)');
      }
      scheduleRebuild();
    });
    watcher.on('error', (err) => {
      console.error(`[astria] Watcher failed:`, err.message || err);
      console.error('[astria] Stopping; restart the watcher to continue.');
      stop(1);
    });
  } catch (err: any) {
    console.error(`Error: Failed to watch "${resolved}": ${err.message || err}`);
    process.exitCode = 1;
    return;
  }

  console.log(`[astria] Watching ${resolved} (debounce: ${debounceMs}ms)`);
  console.log('[astria] Press Ctrl+C to stop');

  const stop = (exitCode = 0) => {
    if (stopped) return;
    stopped = true;
    clearTimers();
    rebuildQueued = false;
    pendingEvents.clear();
    watcher.close();
    process.exitCode = exitCode;
    // This child belongs to this watcher; never terminate process categories.
    activeChild?.kill('SIGTERM');
    rl?.close();
    console.log('\n[astria] Stopped.');
  };
  let rl: import('readline').Interface | undefined;
  process.once('SIGINT', () => stop());
  process.once('SIGTERM', () => stop());
  if (process.platform === 'win32') {
    const readline = require('readline');
    rl = readline.createInterface({ input: process.stdin });
    rl!.on('SIGINT', () => stop());
  }
  // Install the watcher first so edits during startup are queued too.
  pendingEvents.add('(startup reconciliation)');
  runRebuild();
}
