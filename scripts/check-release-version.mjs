import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

export function parseReleaseTag(tag) {
  const match = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(tag);
  if (!match) throw new Error(`Release tag must be vMAJOR.MINOR.PATCH; received ${JSON.stringify(tag)}`);
  return tag.slice(1);
}

export function checkReleaseVersion(tag, packageVersion) {
  const version = parseReleaseTag(tag);
  assert.equal(tag, `v${packageVersion}`, `Release tag ${tag} does not match app version ${packageVersion}`);
  return version;
}

async function main() {
  const tag = process.argv[2];
  const packageJson = JSON.parse(await readFile(new URL("../app/package.json", import.meta.url), "utf8"));
  const version = checkReleaseVersion(tag, packageJson.version);
  process.stdout.write(`${version}\n`);
  if (process.env.GITHUB_OUTPUT) {
    const { appendFile } = await import("node:fs/promises");
    await appendFile(process.env.GITHUB_OUTPUT, `version=${version}\n`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
