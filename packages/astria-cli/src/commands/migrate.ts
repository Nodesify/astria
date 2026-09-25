import { existsSync, mkdirSync, renameSync } from 'fs';
import { join } from 'path';

function homeDir(): string {
  return process.env.USERPROFILE || process.env.HOME || '.';
}

function moveOrReport(from: string, to: string, label: string): boolean {
  if (!existsSync(from)) return false;
  if (existsSync(to)) {
    console.log(`[migrate] ${label}: ${to} already exists — left ${from} untouched. Remove it manually once verified.`);
    return false;
  }
  renameSync(from, to);
  console.log(`[migrate] ${label}: ${from} -> ${to}`);
  return true;
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
  moved += Number(moveOrReport(join(root, '.graphify'), join(root, '.astria'), 'data folder'));
  moved += Number(moveOrReport(join(root, '.graphifyignore'), join(root, '.astriaignore'), 'ignore file'));

  const globalFrom = join(homeDir(), '.nodesify-graphify', 'global.db');
  const globalTo = join(homeDir(), '.astria', 'global.db');
  if (existsSync(globalFrom)) {
    if (existsSync(globalTo)) {
      console.log(`[migrate] global store: ${globalTo} already exists — left ${globalFrom} untouched.`);
    } else {
      mkdirSync(join(homeDir(), '.astria'), { recursive: true });
      renameSync(globalFrom, globalTo);
      console.log(`[migrate] global store: ${globalFrom} -> ${globalTo}`);
      moved += 1;
    }
  }

  if (moved === 0) {
    console.log('[migrate] nothing to migrate (no pre-1.0 layout found).');
  } else {
    console.log(`[migrate] done — ${moved} item(s) moved. Run \`astria update ${root}\` to refresh.`);
  }
}
