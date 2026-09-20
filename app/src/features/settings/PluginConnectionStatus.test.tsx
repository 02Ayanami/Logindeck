import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
const { getStatus } = vi.hoisted(() => ({ getStatus: vi.fn() }));
vi.mock('../../lib/tauri', () => ({ tauri: { getBrowserCaptureSettings: getStatus } }));
import '../../app/i18n';
import { PluginConnectionStatus } from './PluginConnectionStatus';
afterEach(() => { cleanup(); vi.useRealTimers(); getStatus.mockReset(); });
it('expires idle leases and recovers when the heartbeat returns', async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-09-17T12:00:00Z'));
  const settings = { enabled: true, last_connected_at: Date.now() / 1000 };
  getStatus.mockImplementation(async () => ({ ...settings }));
  render(<PluginConnectionStatus />);
  await act(async () => {});
  expect(screen.getByLabelText('Browser extension: Connected')).toBeVisible();
  await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
  expect(screen.getByLabelText('Browser extension: Disconnected')).toBeVisible();
  settings.last_connected_at = Date.now() / 1000;
  await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
  expect(screen.getByLabelText('Browser extension: Connected')).toBeVisible();
  settings.enabled = false;
  await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
  expect(screen.getByLabelText('Browser extension: Disconnected')).toBeVisible();
});
it('shows disconnected if status cannot be read', async () => {
  getStatus.mockRejectedValue(new Error('unavailable'));
  render(<PluginConnectionStatus />);
  expect(await screen.findByLabelText('Browser extension: Disconnected')).toBeVisible();
});
