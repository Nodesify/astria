import * as fs from 'fs';
import * as path from 'path';
import { runPipeline } from '../native';

const CODE_EXTENSIONS = new Set([
  '.py', '.js', '.jsx', '.mjs', '.ts', '.tsx',
  '.rs', '.go', '.java', '.c', '.h', '.cpp', '.cc', '.cxx', '.hpp',
]);

const SKIP_DIRS = new Set(['.astria', 'node_modules', 'target', '.git', 'dist', '__pycache__']);

export async function watchCommand(watchPath: string, opts: { debounce: string }) {
  const debounceMs = parseInt(opts.debounce || '3000', 10);
  const resolved = path.resolve(watchPath);

  if (!fs.existsSync(resolved) || !fs.statSync(resolved).isDirectory()) {
    console.error(`Error: "${resolved}" is not a valid directory`);
    process.exitCode = 1;
    return;
  }

  let changedFiles = new Set<string>();
  let timer: ReturnType<typeof setTimeout> | null = null;

  let watcher: fs.FSWatcher;
  try {
    watcher = fs.watch(resolved, { recursive: true }, (_event, filename) => {
      if (!filename) return;
      const filePath = filename.replace(/\\/g, '/');
      const parts = filePath.split('/');

      if (parts.some((p: string) => SKIP_DIRS.has(p))) return;

      const ext = path.extname(filePath).toLowerCase();
      if (!CODE_EXTENSIONS.has(ext)) return;

      changedFiles.add(filePath);

      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        const batch = [...changedFiles];
        changedFiles.clear();
        console.log(`\n[astria] ${batch.length} file(s) changed, rebuilding...`);
        try {
          const result = runPipeline(resolved);
          console.log(`[astria] Rebuilt: ${result.nodesAdded} nodes, ${result.edgesAdded} edges, ${result.communities} communities`);
        } catch (e: any) {
          console.error(`[astria] Rebuild failed:`, e.message || e);
        }
      }, debounceMs);
    });
  } catch (err: any) {
    console.error(`Error: Failed to watch "${resolved}": ${err.message || err}`);
    process.exitCode = 1;
    return;
  }

  console.log(`[astria] Watching ${resolved} (debounce: ${debounceMs}ms)`);
  console.log('[astria] Press Ctrl+C to stop');

  if (process.platform === 'win32') {
    const readline = require('readline');
    const rl = readline.createInterface({ input: process.stdin });
    rl.on('SIGINT', () => {
      console.log('\n[astria] Stopped.');
      watcher.close();
      rl.close();
      process.exit(0);
    });
  } else {
    process.on('SIGINT', () => {
      console.log('\n[astria] Stopped.');
      watcher.close();
      process.exit(0);
    });
    process.on('SIGTERM', () => {
      console.log('\n[astria] Stopped.');
      watcher.close();
      process.exit(0);
    });
  }
}
