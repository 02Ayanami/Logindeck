import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

test('Tauri wrapper runs without a shell or an executable pnpm shim', () => {
  const result = spawnSync(process.execPath, [fileURLToPath(new URL('./tauri.mjs', import.meta.url)), '--help'], {
    encoding: 'utf8',
    timeout: 30_000,
    windowsHide: true,
  });
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);
  assert.match(result.stdout, /Usage:.*tauri/s);
});
