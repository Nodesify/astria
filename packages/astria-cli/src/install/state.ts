import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { writeTextAtomic } from './atomic';

export type InstallScope = 'project' | 'user';
export interface InstallState { version: 1; platforms: string[]; files: Record<string, string> }
export function installRoot(projectDir: string, scope: InstallScope): string {
  return scope === 'user' ? os.homedir() : path.resolve(projectDir);
}
export function statePath(projectDir: string, scope: InstallScope): string {
  return path.join(installRoot(projectDir, scope), '.astria-install.json');
}
export function readInstallState(projectDir: string, scope: InstallScope): InstallState {
  let text: string;
  try { text = fs.readFileSync(statePath(projectDir, scope), 'utf8'); }
  catch (error: any) { if (error.code === 'ENOENT') return { version: 1, platforms: [], files: {} }; throw error; }
  const value = JSON.parse(text);
  if (value.version !== 1 || !Array.isArray(value.platforms) || !value.platforms.every((p: unknown) => typeof p === 'string') || !value.files || typeof value.files !== 'object' || !Object.values(value.files).every(v => typeof v === 'string')) {
    throw new Error('Invalid Astria installation record; refusing to change shared registrations.');
  }
  return value;
}
export function saveInstallState(projectDir: string, scope: InstallScope, state: InstallState): void {
  if (!state.platforms.length && !Object.keys(state.files).length) {
    try { fs.unlinkSync(statePath(projectDir, scope)); }
    catch (error: any) { if (error.code !== 'ENOENT') throw error; }
    return;
  }
  writeTextAtomic(statePath(projectDir, scope), JSON.stringify(state, null, 2) + '\n');
}
export function parseScope(value: string): InstallScope {
  if (value !== 'project' && value !== 'user') throw new Error('Scope must be project or user.');
  return value;
}
