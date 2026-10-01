import * as fs from 'fs';
import * as path from 'path';

const sleep = (ms: number) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);

/// Write `text` to `filePath` via temp-file + rename so a crash, kill, or
/// disk-full mid-write can never leave a truncated config. On Windows the
/// rename replaces an existing target (libuv passes MOVEFILE_REPLACE_EXISTING);
/// a target locked by another process (editor, antivirus) is retried briefly
/// and finally falls back to a direct write — degraded, but never a failed
/// install over an intact old file.
export function writeTextAtomic(filePath: string, text: string): void {
  const dir = path.dirname(filePath);
  fs.mkdirSync(dir, { recursive: true });
  const tmp = path.join(dir, `.${path.basename(filePath)}.astria-tmp`);
  fs.writeFileSync(tmp, text, 'utf-8');
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      fs.renameSync(tmp, filePath);
      return;
    } catch (e: any) {
      if (attempt === 2) {
        // Last resort: keep the old direct-write behavior rather than fail.
        try { fs.unlinkSync(tmp); } catch { /* absent */ }
        fs.writeFileSync(filePath, text, 'utf-8');
        return;
      }
      sleep(100);
    }
  }
}
