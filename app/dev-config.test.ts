// @vitest-environment node
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const packageJson = JSON.parse(readFileSync('package.json', 'utf8'));
const tauriConfig = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
const workspaceConfig = readFileSync('pnpm-workspace.yaml', 'utf8');
const cargoManifest = readFileSync('src-tauri/Cargo.toml', 'utf8');
const cargoLock = readFileSync('../Cargo.lock', 'utf8');

describe('desktop development configuration', () => {
  it('starts Vite automatically from one desktop command', () => {
    expect(packageJson.scripts.desktop).toBe('node ../scripts/tauri.mjs dev');
    expect(tauriConfig.build.beforeDevCommand).toBe('node ../scripts/prepare-edge-bundle.mjs && pnpm dev');
    expect(tauriConfig.build.devUrl).toBe('http://localhost:1420');
  });

  it('keeps pnpm build approval in the workspace config without deprecated package metadata', () => {
    expect(packageJson).not.toHaveProperty('pnpm');
    expect(workspaceConfig).toContain('allowBuilds:');
    expect(workspaceConfig).toContain('esbuild: true');
  });

  it('keeps every desktop version source synchronized', () => {
    expect(tauriConfig.version).toBe(packageJson.version);
    expect(cargoManifest).toMatch(new RegExp(`^version = "${packageJson.version.replaceAll('.', '\\.')}"$`, 'm'));
    expect(cargoLock).toContain(`[[package]]\nname = "autologin-desktop"\nversion = "${packageJson.version}"\n`);
  });
});
