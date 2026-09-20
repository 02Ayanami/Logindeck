import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { copyNativeHost } from './bundle-native-host.mjs';

for (const [target, platform, expectedName] of [
  [undefined, 'win32', 'autologin-native-host.exe'],
  ['x86_64-pc-windows-msvc', 'darwin', 'autologin-native-host.exe'],
  [undefined, 'darwin', 'autologin-native-host'],
  ['aarch64-apple-darwin', 'win32', 'autologin-native-host'],
]) {
  test(`bundles the compiled native host for ${target ?? platform}`, async () => {
    const fixture = await mkdtemp(path.join(tmpdir(), 'logindeck-bundle-'));
    try {
      const targetDir = path.join(fixture, 'target');
      const release = path.join(targetDir, ...(target ? [target] : []), 'release');
      const resource = path.join(fixture, 'resource');
      await mkdir(release, { recursive: true });
      await mkdir(resource);
      await writeFile(path.join(release, expectedName), 'controlled binary fixture');
      await copyNativeHost(targetDir, target, resource, platform);
      assert.equal(await readFile(path.join(resource, expectedName), 'utf8'), 'controlled binary fixture');
    } finally {
      await rm(fixture, { recursive: true, force: true });
    }
  });
}
