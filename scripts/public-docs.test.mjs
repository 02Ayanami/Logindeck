import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');

test('public repository metadata is complete and honest', () => {
  const license = read('LICENSE');
  const security = read('SECURITY.md');
  const contributing = read('CONTRIBUTING.md');
  const readme = read('README.md');
  const combined = `${license}\n${security}\n${contributing}\n${readme}`;

  assert.match(license, /MIT License/);
  assert.match(license, /LoginDeck contributors/);
  assert.match(security, /Security Advisories/i);
  assert.match(contributing, /Rust 1\.89\.0/);
  assert.match(contributing, /pnpm 11\.19\.0/);
  assert.match(readme, /Windows.*x64/is);
  assert.match(readme, /Apple Silicon|M 系列/i);
  assert.match(readme, /未签名|unsigned/i);
  assert.match(readme, /加载解压缩|Load unpacked/i);
  assert.doesNotMatch(combined, /\b(?:TBD|TODO|PLACEHOLDER)\b/);
});
