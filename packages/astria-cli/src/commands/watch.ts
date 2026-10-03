import * as fs from 'fs';
import * as path from 'path';
import { spawn } from 'child_process';

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

export async function watchCommand(watchPath: string, opts: { debounce: string }) {
  const debounceMs = Number(opts.debounce || '3000');
  if (!Number.isFinite(debounceMs) || debounceMs <= 0) {
    console.error(`Error: invalid --debounce value "${opts.debounce}" (must be a positive number of milliseconds)`);
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

  const runRebuild = () => {
    if (rebuildInFlight) {
      rebuildQueued = true;
      return;
    }
    rebuildInFlight = true;
    console.log(`\n[astria] ${pendingEvents.size} change event(s), rebuilding...`);
    pendingEvents.clear();
    const child = spawn(
      process.execPath,
      [process.argv[1], 'update', resolved],
      { stdio: 'inherit' },
    );
    child.on('error', (err) => {
      console.error('[astria] Rebuild could not start:', err.message);
    });
    child.on('exit', (code) => {
      if (code !== 0) {
        console.error(`[astria] Rebuild exited with code ${code}`);
      }
      rebuildInFlight = false;
      if (rebuildQueued) {
        rebuildQueued = false;
        runRebuild();
      }
    });
  };

  const scheduleRebuild = () => {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(runRebuild, debounceMs);
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
      watcher.close();
      process.exitCode = 1;
    });
  } catch (err: any) {
    console.error(`Error: Failed to watch "${resolved}": ${err.message || err}`);
    process.exitCode = 1;
    return;
  }

  console.log(`[astria] Watching ${resolved} (debounce: ${debounceMs}ms)`);
  console.log('[astria] Press Ctrl+C to stop');

  const stop = () => {
    console.log('\n[astria] Stopped.');
    watcher.close();
    process.exit(0);
  };
  if (process.platform === 'win32') {
    const readline = require('readline');
    const rl = readline.createInterface({ input: process.stdin });
    rl.on('SIGINT', stop);
  } else {
    process.on('SIGINT', stop);
    process.on('SIGTERM', stop);
  }
}
