import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = (path) => readFile(new URL(`../${path}`, import.meta.url), "utf8");

test("platform verification workflows are least-privilege and architecture-specific", async () => {
  const [windows, macos] = await Promise.all([
    read(".github/workflows/windows.yml"),
    read(".github/workflows/macos.yml"),
  ]);

  for (const workflow of [windows, macos]) {
    assert.match(workflow, /pull_request:\s*\n\s+branches: \[main\]/);
    assert.match(workflow, /push:\s*\n\s+branches: \[main\]/);
    assert.match(workflow, /permissions:\s*\n\s+contents: read/);
    assert.doesNotMatch(workflow, /contents: write/);
  }

  assert.match(windows, /runs-on: windows-2022/);
  assert.match(windows, /verify-windows\.ps1/);

  assert.match(macos, /runs-on: macos-14/);
  assert.match(macos, /uname -m/);
  assert.match(macos, /arm64/);
  assert.match(macos, /verify-macos\.sh/);
});

test("release workflow publishes only after both native installers exist", async () => {
  const release = await read(".github/workflows/release.yml");
  assert.match(release, /tags: \["v\*"\]/);
  assert.match(release, /check-release-version\.mjs/);
  assert.match(release, /runs-on: windows-2022/);
  assert.match(release, /runs-on: macos-14/);
  assert.match(release, /tauri build --bundles nsis/);
  assert.match(release, /tauri build --bundles dmg/);
  assert.match(release, /needs: \[prepare, windows, macos\]/);
  assert.match(release, /LoginDeck-\$\{\{ needs\.prepare\.outputs\.version \}\}-windows-x64-setup\.exe/);
  assert.match(release, /LoginDeck-\$\{\{ needs\.prepare\.outputs\.version \}\}-macos-arm64\.dmg/);
  assert.match(release, /SHA256SUMS\.txt/);

  const writePermissions = release.match(/contents: write/g) ?? [];
  assert.equal(writePermissions.length, 1);
  assert.match(release, /release:[\s\S]*?permissions:\s*\n\s+contents: write/);
});
