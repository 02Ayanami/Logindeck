import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const appDir = path.join(root, 'app');
const packagePath = path.join(appDir, 'package.json');
const tauriPath = path.join(appDir, 'src-tauri', 'tauri.conf.json');
const cargoPath = path.join(appDir, 'src-tauri', 'Cargo.toml');
const cargoLockPath = path.join(root, 'Cargo.lock');

const packageJson = JSON.parse(await readFile(packagePath, 'utf8'));
const version = packageJson.version;
if (!/^\d+\.\d+\.\d+$/.test(version)) {
  throw new Error(`Invalid application version: ${version}`);
}

const tauri = JSON.parse(await readFile(tauriPath, 'utf8'));
if (tauri.version !== version) {
  tauri.version = version;
  await writeFile(tauriPath, `${JSON.stringify(tauri, null, 2)}\n`);
}

let cargo = await readFile(cargoPath, 'utf8');
const match = cargo.match(/^(version\s*=\s*")[^"]+("\s*)$/m);
if (!match) throw new Error(`Missing package version in ${cargoPath}`);
const next = `${match[1]}${version}${match[2]}`;
if (match[0] !== next) {
  cargo = cargo.replace(match[0], next);
  await writeFile(cargoPath, cargo);
}

let cargoLock = await readFile(cargoLockPath, 'utf8');
const lockPattern = /(\[\[package\]\]\r?\nname = "autologin-desktop"\r?\nversion = ")[^"]+("\r?\n)/;
const lockMatch = cargoLock.match(lockPattern);
if (!lockMatch) throw new Error(`Missing autologin-desktop package in ${cargoLockPath}`);
const nextLock = `${lockMatch[1]}${version}${lockMatch[2]}`;
if (lockMatch[0] !== nextLock) {
  cargoLock = cargoLock.replace(lockPattern, nextLock);
  await writeFile(cargoLockPath, cargoLock);
}

console.log(`LoginDeck version: ${version}`);
