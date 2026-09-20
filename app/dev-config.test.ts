// @vitest-environment node
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const packageJson = JSON.parse(readFileSync('package.json', 'utf8'));
const tauriConfig = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
const workspaceConfig = readFileSync('pnpm-workspace.yaml', 'utf8');

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
});
