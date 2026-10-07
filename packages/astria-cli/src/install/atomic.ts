import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { randomUUID, createHash } from 'crypto';
const sleep = (ms: number) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);

/** Serialize the entire read/modify/write operation, including shared user configs. */
export function withInstallLock<T>(work: () => T): T {
  const user = createHash('sha256').update(os.homedir()).digest('hex').slice(0, 20);
  const lock = path.join(os.tmpdir(), `astria-install-${user}.lock`);
  let fd: number;
  try { fd = fs.openSync(lock, 'wx', 0o600); }
  catch (error: any) {
    if (error.code !== 'EEXIST') throw error;
    throw new Error(`Installation is locked: ${lock}. Wait for the other install to finish. If it crashed, verify the recorded PID is no longer running before removing this lock.`);
  }
  try {
    fs.writeFileSync(fd, JSON.stringify({ pid: process.pid, started: new Date().toISOString() }));
    return work();
  } finally { fs.closeSync(fd); fs.unlinkSync(lock); }
}

/** A failed replacement never truncates the original. */
export function writeTextAtomic(filePath: string, text: string): void {
  const dir = path.dirname(filePath);
  fs.mkdirSync(dir, { recursive: true });
  const tmp = path.join(dir, `.${path.basename(filePath)}.${randomUUID()}.astria-tmp`);
  let mode = 0o600;
  try { mode = fs.statSync(filePath).mode; } catch (error: any) { if (error.code !== 'ENOENT') throw error; }
  const fd = fs.openSync(tmp, 'wx', mode);
  try {
    try { fs.writeFileSync(fd, text, 'utf-8'); fs.fsyncSync(fd); }
    finally { fs.closeSync(fd); }
    for (let attempt = 0; ; attempt++) {
      try { fs.renameSync(tmp, filePath); return; }
      catch (error) { if (attempt === 2) throw error; sleep(100); }
    }
  } finally { try { fs.unlinkSync(tmp); } catch (error: any) { if (error.code !== 'ENOENT') throw error; } }
}
