import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const app = path.join(root, 'app');
const cargo = process.env.CARGO ?? 'cargo';

function run(name, command, args, cwd = root, env = {}) {
  console.log(`\n=== ${name} ===`);
  console.log(`$ ${command} ${args.join(' ')}`);
  const result = spawnSync(command, args, {
    cwd,
    stdio: 'inherit',
    env: { ...process.env, ...env },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

if (process.platform !== 'darwin') {
  throw new Error('LoginDeck macOS preflight must run on macOS');
}

run('Sync versions', 'pnpm', ['sync-version'], app);
run('Rust format', cargo, ['fmt', '--all', '--', '--check']);
run('Rust tests', cargo, ['test', '--workspace', '--locked']);
run('Frontend tests', 'pnpm', ['test', '--run'], app);
run('Frontend build', 'pnpm', ['build'], app);
run('Edge extension tests', 'node', ['--test', 'tests/*.test.mjs'], path.join(root, 'browser-extension'));
run('Edge extension build', 'node', ['build.mjs'], path.join(root, 'browser-extension'));
run('macOS resource bundle', 'node', ['scripts/prepare-edge-bundle.mjs']);
run('Diff check', 'git', ['diff', '--check']);

console.log('\n[OK] LoginDeck macOS preflight completed.');
