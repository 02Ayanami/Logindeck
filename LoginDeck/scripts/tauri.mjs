import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);

const child = spawn('pnpm', ['sync-version'], { cwd: path.join(root, 'app'), stdio: 'inherit' });
child.on('error', (error) => { throw error; });
child.on('exit', (code, signal) => {
  if (code !== 0) process.exit(code ?? 1);
  const tauri = spawn('pnpm', ['exec', 'tauri', ...args], { cwd: path.join(root, 'app'), stdio: 'inherit' });
  tauri.on('error', (error) => { throw error; });
  tauri.on('exit', (tauriCode, tauriSignal) => {
    if (tauriSignal) process.kill(process.pid, tauriSignal);
    process.exit(tauriCode ?? 1);
  });
});
