import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';

const { getBrowserCaptureSettings, setBrowserCaptureEnabled } = vi.hoisted(() => ({
  getBrowserCaptureSettings: vi.fn(),
  setBrowserCaptureEnabled: vi.fn(),
}));

vi.mock('../../lib/tauri', () => ({
  tauri: {
    getBrowserCaptureSettings,
    setBrowserCaptureEnabled,
    setLocale: vi.fn(),
  },
}));

import '../../app/i18n';
import { SettingsPage } from './SettingsPage';

beforeEach(() => {
  getBrowserCaptureSettings.mockReset();
  setBrowserCaptureEnabled.mockReset();
  getBrowserCaptureSettings.mockResolvedValue({
    enabled: false,
    revision: 0,
    last_connected_at: null,
  });
  setBrowserCaptureEnabled.mockResolvedValue({
    enabled: true,
    revision: 1,
    last_connected_at: null,
  });
});

it('matches the current Mac settings structure', async () => {
  render(<SettingsPage />);

  expect(screen.getByRole('heading', { name: 'Settings' })).toBeVisible();
  expect(screen.getByRole('heading', { name: 'Language' })).toBeVisible();
  expect(await screen.findByRole('heading', { name: 'Edge login detection' })).toBeVisible();
  expect(screen.getByRole('switch', { name: 'Edge login detection' })).toHaveAttribute(
    'aria-checked',
    'false',
  );
  expect(screen.getByRole('button', { name: 'Installation and usage help' })).toBeVisible();
  expect(screen.queryByRole('heading', { name: 'Security' })).toBeNull();
  expect(screen.queryByRole('heading', { name: 'About' })).toBeNull();
});

it('persists Edge login detection from the Settings switch', async () => {
  render(<SettingsPage />);

  const control = await screen.findByRole('switch', { name: 'Edge login detection' });
  await userEvent.click(control);

  expect(setBrowserCaptureEnabled).toHaveBeenCalledWith(true);
  expect(control).toHaveAttribute('aria-checked', 'true');
});
