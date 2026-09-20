import { createHash } from "node:crypto";
import { appendFile, copyFile, lstat, mkdir, readdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const definitions = {
  windows: {
    match: (version) => new RegExp(`^LoginDeck_${version.replaceAll(".", "\\.")}_x64-setup\\.exe$`),
    filename: (version) => `LoginDeck-${version}-windows-x64-setup.exe`,
  },
  macos: {
    match: (version) => new RegExp(`^LoginDeck_${version.replaceAll(".", "\\.")}_(?:aarch64|arm64)\\.dmg$`),
    filename: (version) => `LoginDeck-${version}-macos-arm64.dmg`,
  },
};

export async function collectReleaseAsset({ platform, version, source, output }) {
  const definition = definitions[platform];
  if (!definition) throw new Error(`Unsupported release platform: ${platform}`);
  const sourcePath = resolve(source);
  const entries = await readdir(sourcePath, { withFileTypes: true });
  const candidates = entries.filter((entry) => entry.isFile() && definition.match(version).test(entry.name));
  if (candidates.length !== 1) {
    throw new Error(`Expected exactly one ${platform} installer for ${version}; found ${candidates.length}`);
  }

  const input = resolve(sourcePath, candidates[0].name);
  const stat = await lstat(input);
  if (!stat.isFile() || stat.isSymbolicLink()) throw new Error("Release installer must be a regular file");
  const bytes = await readFile(input);
  const filename = definition.filename(version);
  const outputPath = resolve(output);
  await mkdir(outputPath, { recursive: true });
  const destination = resolve(outputPath, filename);
  await copyFile(input, destination);
  return { filename, path: destination, sha256: createHash("sha256").update(bytes).digest("hex") };
}

async function main() {
  const [platform, version, source, output] = process.argv.slice(2);
  const result = await collectReleaseAsset({ platform, version, source, output });
  process.stdout.write(`${result.filename}\n${result.sha256}  ${result.filename}\n`);
  if (process.env.GITHUB_OUTPUT) {
    await appendFile(process.env.GITHUB_OUTPUT, `filename=${result.filename}\npath=${result.path}\nsha256=${result.sha256}\n`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
