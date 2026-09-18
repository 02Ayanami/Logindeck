// @vitest-environment node
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import viteConfig from './vite.config';

type TauriConfig = {
  build: {
    devUrl: string;
  };
};

const tauriConfig = JSON.parse(
  readFileSync(new URL('./src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
) as TauriConfig;

describe('development endpoints', () => {
  it('uses the Tauri development URL port without falling back', () => {
    const tauriPort = Number(new URL(tauriConfig.build.devUrl).port);

    expect(viteConfig.server?.port).toBe(tauriPort);
    expect(viteConfig.server?.strictPort).toBe(true);
  });
});
