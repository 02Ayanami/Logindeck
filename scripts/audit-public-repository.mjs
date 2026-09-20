import { execFileSync, spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const MAX_TEXT_BYTES = 10 * 1024 * 1024;
const LARGE_FILE_BYTES = 10 * 1024 * 1024;
const syntheticUsers = new Set(["a", "alice", "demo", "example", "fixture", "test", "tester", "agent"]);

const secretPatterns = [
  /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/,
  /\bghp_[A-Za-z0-9]{30,}\b/,
  /\bgithub_pat_[A-Za-z0-9_]{30,}\b/,
  /\bAKIA[0-9A-Z]{16}\b/,
  /\bsk-[A-Za-z0-9_-]{24,}\b/,
];

export function auditPath(path) {
  const findings = [];
  const normalized = path.replaceAll("\\", "/");
  const base = normalized.split("/").at(-1) ?? normalized;
  if (
    /(^|\/)\.env(?:\.|$)/i.test(normalized) ||
    /\.(?:pem|p12|pfx|key|sqlite|sqlite3|db)$/i.test(base) ||
    /(?:credential|secret|signing)[^/]*\.(?:json|txt|bin)$/i.test(base)
  ) findings.push("sensitive-file");
  if (/\.(?:exe|msi|dmg|pkg|appx|msix)$/i.test(base)) findings.push("generated-installer");
  return findings;
}

export function auditText(text) {
  const findings = [];
  if (secretPatterns.some((pattern) => pattern.test(text))) findings.push("secret-pattern");

  const windows = /\b[A-Za-z]:\\Users\\([^\\\s`"']+)/g;
  const macos = /\/Users\/([^/\s`"']+)/g;
  const users = [...text.matchAll(windows), ...text.matchAll(macos)].map((match) => match[1].toLowerCase());
  if (users.some((user) => !syntheticUsers.has(user))) findings.push("local-absolute-path");
  return findings;
}

function trackedPaths(cwd) {
  return execFileSync("git", ["ls-files", "-z"], { cwd }).toString("utf8").split("\0").filter(Boolean);
}

function historyObjects(cwd) {
  const lines = execFileSync("git", ["rev-list", "--objects", "--all"], { cwd, maxBuffer: 64 * 1024 * 1024 })
    .toString("utf8").split(/\r?\n/).filter(Boolean);
  const paths = new Map();
  for (const line of lines) {
    const separator = line.indexOf(" ");
    const hash = separator < 0 ? line : line.slice(0, separator);
    if (separator >= 0 && !paths.has(hash)) paths.set(hash, line.slice(separator + 1));
  }
  const hashes = [...new Set(lines.map((line) => line.split(" ", 1)[0]))];
  if (!hashes.length) return [];
  const input = `${hashes.join("\n")}\n`;
  const checked = spawnSync("git", ["cat-file", "--batch-check=%(objectname) %(objecttype) %(objectsize)"], {
    cwd, input, encoding: "utf8", maxBuffer: 64 * 1024 * 1024,
  });
  if (checked.status !== 0) throw new Error("Unable to inspect Git history objects");
  return checked.stdout.split(/\r?\n/).filter(Boolean).map((line) => {
    const [hash, type, size] = line.split(" ");
    return { hash, type, size: Number(size), path: paths.get(hash) ?? `(object ${hash.slice(0, 12)})` };
  });
}

function readBlob(cwd, hash, size) {
  if (size > MAX_TEXT_BYTES) return null;
  const result = spawnSync("git", ["cat-file", "blob", hash], { cwd, encoding: null, maxBuffer: MAX_TEXT_BYTES + 1024 });
  if (result.status !== 0 || result.stdout.includes(0)) return null;
  return result.stdout.toString("utf8");
}

export function auditRepository(cwd = process.cwd()) {
  const findings = [];
  const add = (scope, path, category) => findings.push({ scope, path, category });

  for (const path of trackedPaths(cwd)) {
    for (const category of auditPath(path)) add("current", path, category);
    const bytes = readFileSync(join(cwd, path));
    if (bytes.length > LARGE_FILE_BYTES) add("current", path, "large-file");
    if (bytes.length <= MAX_TEXT_BYTES && !bytes.includes(0)) {
      for (const category of auditText(bytes.toString("utf8"))) add("current", path, category);
    }
  }

  for (const object of historyObjects(cwd)) {
    if (object.type !== "blob") continue;
    for (const category of auditPath(object.path)) add("history", object.path, category);
    if (object.size > LARGE_FILE_BYTES) add("history", object.path, "large-file");
    const text = readBlob(cwd, object.hash, object.size);
    if (text !== null) for (const category of auditText(text)) add("history", object.path, category);
  }

  return [...new Map(findings.map((finding) => [`${finding.scope}:${finding.path}:${finding.category}`, finding])).values()];
}

function main() {
  const findings = auditRepository();
  const summary = new Map();
  for (const finding of findings) {
    console.log(`${finding.scope}\t${finding.category}\t${finding.path}`);
    const key = `${finding.scope}:${finding.category}`;
    summary.set(key, (summary.get(key) ?? 0) + 1);
  }
  console.log(`Audit findings: ${findings.length}`);
  for (const [key, count] of [...summary].sort()) console.log(`${key}: ${count}`);
  process.exitCode = findings.some((finding) => finding.category === "secret-pattern" || finding.category === "sensitive-file") ? 1 : 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
