import { existsSync, mkdirSync, renameSync } from 'fs';
import { join } from 'path';

function homeDir(): string {
  return process.env.USERPROFILE || process.env.HOME || '.';
}

type MoveResult = 'moved' | 'absent' | 'target-exists' | 'locked';

function moveOrReport(from: string, to: string, label: string): MoveResult {
  if (!existsSync(from)) return 'absent';
  if (existsSync(to)) {
    console.log(`[migrate] ${label}: ${to} already exists — left ${from} untouched. Remove it manually once verified.`);
    return 'target-exists';
  }
  try {
    renameSync(from, to);
  } catch (err: any) {
    if (err && (err.code === 'EPERM' || err.code === 'EBUSY' || err.code === 'ENOTEMPTY')) {
      console.error(
        `[migrate] ${label}: ${from} is locked by another process (editor MCP server, watcher, or a running query).\n` +
        `Close it and re-run \`astria migrate\`.`
      );
      return 'locked';
    }
    throw err;
  }
  console.log(`[migrate] ${label}: ${from} -> ${to}`);
  return 'moved';
}

/// One-time migration from the pre-1.0 nodesify-graphify layout. 1.0 reads
/// only `.astria/`, so this is the escape hatch for existing graphs: the
/// data folder, ignore file, and home-dir global store are renamed in place.
export function migrateCommand(opts: { graph: string }) {
  const root = opts.graph;
  if (!existsSync(root)) {
    throw new Error(`path not found: ${root}`);
  }

  let moved = 0;
  let locked = 0;
  const results = [
    moveOrReport(join(root, '.graphify'), join(root, '.astria'), 'data folder'),
    moveOrReport(join(root, '.graphifyignore'), join(root, '.astriaignore'), 'ignore file'),
  ];
  for (const r of results) {
    if (r === 'moved') moved += 1;
    if (r === 'locked') locked += 1;
  }

  const globalFrom = join(homeDir(), '.nodesify-graphify', 'global.db');
  const globalTo = join(homeDir(), '.astria', 'global.db');
  if (existsSync(globalFrom)) {
    if (existsSync(globalTo)) {
      console.log(`[migrate] global store: ${globalTo} already exists — left ${globalFrom} untouched.`);
    } else {
      const r = moveOrReport(globalFrom, globalTo, 'global store');
      if (r === 'moved') moved += 1;
      if (r === 'locked') locked += 1;
    }
  }

  if (locked > 0) {
    console.log(`[migrate] ${locked} item(s) still locked — close the locking process and re-run \`astria migrate\`.`);
    process.exitCode = 1;
  } else if (moved === 0) {
    console.log('[migrate] nothing to migrate (no pre-1.0 layout found).');
  } else {
    console.log(`[migrate] done — ${moved} item(s) moved. Run \`astria update ${root}\` to refresh.`);
  }
}
