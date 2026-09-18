// Developer build step. End users only need the resulting macOS application bundle.
import { spawnSync } from 'node:child_process';
import { cp, mkdir, rm } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
const root = fileURLToPath(new URL('..', import.meta.url));
function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: 'inherit' });
  if (result.error || result.status !== 0) throw result.error ?? new Error(`${command} failed`);
}
run(process.execPath, ['browser-extension/build.mjs']);
const target = process.env.TAURI_ENV_TARGET_TRIPLE;
run('cargo', ['build', '--locked', '-p', 'autologin-native-host', '--release', ...(target ? ['--target', target] : [])]);
const resource = path.join(root, 'app/src-tauri/resources/edge');
await rm(resource, { recursive: true, force: true });
await mkdir(resource, { recursive: true });
const targetDir = path.resolve(root, process.env.CARGO_TARGET_DIR ?? 'target');
const hostName = 'autologin-native-host';
await cp(path.join(targetDir, ...(target ? [target] : []), 'release', hostName), path.join(resource, hostName));
await cp(path.join(root, 'browser-extension/dist'), path.join(resource, 'extension'), { recursive: true });
console.log('Bundled Edge installation resources prepared.');
