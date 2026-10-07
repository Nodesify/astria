import { createHash, randomUUID } from 'node:crypto';
import { existsSync, readFileSync, realpathSync, writeFileSync, renameSync, statSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

export const sha = value => createHash('sha256').update(value).digest('hex');
export const readJson = file => JSON.parse(readFileSync(file, 'utf8'));
export const hashFile = file => sha(readFileSync(file));
export function executable(command) {
  const extensions = process.platform === 'win32' ? ['', '.exe', '.com'] : [''];
  const folders = path.isAbsolute(command) || command.includes(path.sep) ? [''] : (process.env.PATH ?? '').split(path.delimiter);
  for (const folder of folders) for (const extension of extensions) {
    const candidate = folder ? path.join(folder, command + extension) : path.resolve(command + extension);
    if (existsSync(candidate) && statSync(candidate).isFile()) return canonical(candidate);
  }
  throw Error('Agent executable is unavailable');
}
export function save(file, value) {
  const temp = `${file}.${randomUUID()}.tmp`;
  writeFileSync(temp, JSON.stringify(value, null, 2) + '\n', { flag: 'wx' });
  renameSync(temp, file);
}
export function assert(condition, message) { if (!condition) throw Error(message); }
export function canonical(file) {
  const resolved = realpathSync(file);
  return process.platform === 'win32' ? resolved.toLowerCase() : resolved;
}
export function overlaps(a, b) {
  const inside = (parent, child) => {
    const relative = path.relative(parent, child);
    return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
  };
  return inside(a, b) || inside(b, a);
}
export function git(root, ...args) {
  const result = spawnSync('git', ['-C', root, ...args], { encoding: 'utf8', shell: false, timeout: 30_000 });
  assert(result.status === 0, `Git inspection failed (${args[0]})`);
  return result.stdout.trim();
}
export function dirty(root) {
  return git(root, 'status', '--porcelain', '--untracked-files=all').split('\n')
    .filter(line => line && !/^\?\? \.astria\//.test(line));
}
export function evidence(root, names) {
  if (!Array.isArray(names) || !names.length) return null;
  return names.map(name => {
    assert(typeof name === 'string' && !path.isAbsolute(name) && !name.split(/[\\/]/).includes('..'), 'Evidence must use relative paths');
    const file = path.join(root, name);
    assert(existsSync(file) && overlaps(canonical(root), canonical(file)) && canonical(file) !== canonical(root), 'Missing or escaped evidence');
    return { path: name, sha256: hashFile(file) };
  });
}
export function measurement(root, entry) {
  if (entry == null) return { value: null, evidence: null };
  assert(Number.isFinite(entry.value) && entry.value >= 0, 'Measurements must be nonnegative numbers');
  const proof = evidence(root, entry.evidence);
  assert(proof, 'A measurement requires evidence; omit unknown measurements');
  return { value: entry.value, evidence: proof };
}
