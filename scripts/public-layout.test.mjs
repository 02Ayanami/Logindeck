import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';

const root = process.cwd();

test('public repository files live at the Git root', () => {
  for (const relative of [
    'Cargo.toml',
    'README.md',
    'app/package.json',
    '.github/workflows/windows.yml',
    '.github/workflows/macos.yml',
  ]) {
    assert.equal(existsSync(resolve(root, relative)), true, `missing root path: ${relative}`);
  }
  assert.equal(existsSync(resolve(root, 'LoginDeck/Cargo.toml')), false, 'nested product wrapper remains');
});
