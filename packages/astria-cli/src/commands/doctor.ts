import * as fs from 'fs';
import * as path from 'path';
import { embeddingsSupported, getPlatformSuffix, graphBuildInfo } from '../native';
import { installRoot, parseScope, readInstallState } from '../install/state';
import { PLATFORMS, PLATFORM_NAMES } from '../install/platforms';
import { inspectAgentMcp } from '../install/settings-inject';
import { statusGitHooks } from '../install/hooks';
import { createHash } from 'crypto';

type Check = { name: string; status: 'ok' | 'warning' | 'error'; detail: string };

function executableOnPath(name: string): string | undefined {
  const extensions = process.platform === 'win32' ? (process.env.PATHEXT ?? '.EXE;.CMD;.BAT').split(';') : [''];
  for (const dir of (process.env.PATH ?? '').split(path.delimiter)) {
    if (!dir) continue;
    for (const extension of extensions) {
      const file = path.join(dir.replace(/^"|"$/g, ''), name + extension);
      try { fs.accessSync(file, process.platform === 'win32' ? fs.constants.R_OK : fs.constants.X_OK); if (fs.statSync(file).isFile()) return file; }
      catch { /* Try next PATH entry. */ }
    }
  }
  return undefined;
}

export function doctorCommand(opts: { graph: string; scope: string; json?: boolean }): void {
  const checks: Check[] = [];
  const add = (name: string, status: Check['status'], detail: string) => checks.push({ name, status, detail });
  const check = (name: string, work: () => void) => { try { work(); } catch (error: any) { add(name, 'error', error.message); } };
  check('node', () => add('node', Number(process.versions.node.split('.')[0]) >= 22 ? 'ok' : 'error', `${process.version} at ${process.execPath}; requires Node 22+`));
  check('native', () => {
    const embed = embeddingsSupported();
    add('native', 'ok', `Loaded ${getPlatformSuffix()}`);
    add('embeddings', embed ? 'ok' : 'warning', embed ? 'Local embeddings supported; doctor does not download the model' : 'This build has no local embedding runtime; omit --embed');
  });
  const executable = executableOnPath('astria');
  add('executable', executable ? 'ok' : 'error', executable ? `PATH resolves to ${executable}; check the editor uses this same PATH and restart it after upgrades` : 'astria is absent from PATH; install globally or expose the local node_modules/.bin directory to the editor');
  check('installation', () => {
    const scope = parseScope(opts.scope);
    const root = installRoot(opts.graph, scope);
    const state = readInstallState(opts.graph, scope);
    add('installation', state.platforms.length ? 'ok' : 'warning', `${scope}: ${state.platforms.join(', ') || 'no recorded integrations'}`);
    for (const platform of state.platforms.length ? state.platforms : PLATFORM_NAMES) {
      const cfg = PLATFORMS[platform];
      if (!cfg) { add(platform, 'error', 'Unknown platform in installation record'); continue; }
      if (cfg.mcp && (scope === 'project' || platform === 'codex')) check(`${platform} MCP`, () => {
        const result = inspectAgentMcp(root, cfg.mcp!);
        if (!result.present && !state.platforms.includes(platform)) return;
        add(`${platform} MCP`, !result.present ? 'error' : result.managed ? 'ok' : 'warning', `${result.file}: ${result.present ? (result.managed ? 'registered' : 'customized; inspect command and arguments') : 'missing; rerun install'}`);
      });
    }
    for (const [file, expected] of Object.entries(state.files)) check('managed file', () => {
      const actual = createHash('sha256').update(fs.readFileSync(file, 'utf8')).digest('hex');
      add('managed file', actual === expected ? 'ok' : 'warning', `${file}: ${actual === expected ? 'intact' : 'customized; install/uninstall will preserve it and request manual resolution'}`);
    });
    let existing = root;
    while (!fs.existsSync(existing) && path.dirname(existing) !== existing) existing = path.dirname(existing);
    fs.accessSync(existing, fs.constants.W_OK);
    for (const file of Object.keys(state.files)) {
      let target = file;
      while (!fs.existsSync(target) && path.dirname(target) !== target) target = path.dirname(target);
      fs.accessSync(target, fs.constants.W_OK);
    }
    add('permissions', 'ok', `${existing} is writable; editor sandbox permissions may differ`);
  });
  check('git hooks', () => {
    const hooks = statusGitHooks(path.resolve(opts.graph));
    add('git hooks', hooks.some(h => /unsafe|legacy|Not a git/.test(h)) ? 'warning' : 'ok', hooks.join('; '));
  });
  check('graph', () => {
    if (!fs.existsSync(path.join(opts.graph, '.astria', 'db.sqlite'))) { add('graph', 'warning', 'No graph; run astria run . --backend none'); return; }
    const info = graphBuildInfo(path.resolve(opts.graph));
    const outdated = info.extractionHashVersion !== info.currentExtractionHashVersion;
    add('graph', outdated ? 'warning' : 'ok', outdated ? 'Extraction rules changed; run astria update .' : `Graph built by Astria ${info.astriaVersion ?? 'unknown'}`);
    if (info.staleExternalIndexes.length) add('external indexes', 'warning', `Reimport: ${info.staleExternalIndexes.join(', ')}`);
  });
  const ok = !checks.some(c => c.status === 'error');
  if (opts.json) console.log(JSON.stringify({ ok, checks }, null, 2));
  else for (const c of checks) console.log(`${c.status.toUpperCase()} ${c.name}: ${c.detail}`);
  if (!ok) process.exitCode = 1;
}
