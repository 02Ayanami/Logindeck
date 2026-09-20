import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { checkReleaseVersion, parseReleaseTag } from "./check-release-version.mjs";
import { collectReleaseAsset } from "./collect-release-assets.mjs";

test("release tags use exact semantic versions and match the package", () => {
  assert.equal(parseReleaseTag("v1.2.3"), "1.2.3");
  for (const invalid of ["1.2.3", "v1.2", "v1.2.3-beta.1", "v01.2.3"]) {
    assert.throws(() => parseReleaseTag(invalid));
  }
  assert.equal(checkReleaseVersion("v0.1.0", "0.1.0"), "0.1.0");
  assert.throws(() => checkReleaseVersion("v0.1.1", "0.1.0"), /does not match/);
});

test("collector publishes one Windows NSIS installer with a stable name", async () => {
  const root = await mkdtemp(join(tmpdir(), "logindeck-release-"));
  const source = join(root, "nsis");
  const output = join(root, "out");
  await mkdir(source);
  await writeFile(join(source, "LoginDeck_0.1.0_x64-setup.exe"), "installer");

  const result = await collectReleaseAsset({ platform: "windows", version: "0.1.0", source, output });
  assert.equal(result.filename, "LoginDeck-0.1.0-windows-x64-setup.exe");
  assert.match(result.sha256, /^[a-f0-9]{64}$/);
  assert.equal(await readFile(result.path, "utf8"), "installer");
});

test("collector publishes one Apple Silicon DMG and rejects ambiguous or wrong assets", async () => {
  const root = await mkdtemp(join(tmpdir(), "logindeck-release-"));
  const source = join(root, "dmg");
  const output = join(root, "out");
  await mkdir(source);
  await writeFile(join(source, "LoginDeck_0.1.0_aarch64.dmg"), "dmg");

  const result = await collectReleaseAsset({ platform: "macos", version: "0.1.0", source, output });
  assert.equal(result.filename, "LoginDeck-0.1.0-macos-arm64.dmg");

  await writeFile(join(source, "LoginDeck_0.1.0_arm64.dmg"), "duplicate");
  await assert.rejects(
    collectReleaseAsset({ platform: "macos", version: "0.1.0", source, output }),
    /exactly one/,
  );
});

test("collector never accepts MSI, debug, raw app, or symbolic-link inputs", async () => {
  const root = await mkdtemp(join(tmpdir(), "logindeck-release-"));
  const source = join(root, "assets");
  const output = join(root, "out");
  await mkdir(source);
  await writeFile(join(source, "LoginDeck_0.1.0_x64_en-US.msi"), "msi");
  await writeFile(join(source, "LoginDeck.exe"), "exe");
  await assert.rejects(
    collectReleaseAsset({ platform: "windows", version: "0.1.0", source, output }),
    /exactly one/,
  );
});
