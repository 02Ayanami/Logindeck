import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
const appDir = path.join(root, 'app');
const require = createRequire(path.join(appDir, 'package.json'));

// Invoke Node entry points directly: Windows package-manager shims are .cmd files,
// and shell interpretation would also alter user-supplied Tauri arguments.
const child = spawn(process.execPath, [path.join(root, 'scripts/sync-version.mjs')], { cwd: appDir, stdio: 'inherit', windowsHide: true });
child.on('error', (error) => { throw error; });
child.on('exit', (code, signal) => {
  if (code !== 0) process.exit(code ?? 1);
  const tauri = spawn(process.execPath, [require.resolve('@tauri-apps/cli/tauri.js'), ...args], { cwd: appDir, stdio: 'inherit', windowsHide: true });
  tauri.on('error', (error) => { throw error; });
  tauri.on('exit', (tauriCode, tauriSignal) => {
    if (tauriSignal) process.kill(process.pid, tauriSignal);
    process.exit(tauriCode ?? 1);
  });
});
