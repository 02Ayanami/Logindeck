import { act, render, screen, waitFor } from '@testing-library/react';
import { StrictMode } from 'react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
import '../../app/i18n';
import { ApplicationLibrary } from './ApplicationLibrary';

it('does not accept an obsolete initial response after effect restart', async () => {
  let resolveOld!: (value: unknown) => void;
  let lists = 0;
  invoke.mockImplementation(async name => {
    if (name === 'list_applications') {
      if (++lists === 1) return new Promise(resolve => { resolveOld = resolve; });
      return [];
    }
    if (name === 'get_scan_status') return { id: 0, phase: 'idle', started_at: null, count: null, error: null };
    return null;
  });
  render(<StrictMode><ApplicationLibrary /></StrictMode>);
  await screen.findByRole('heading', { name: 'Add your everyday apps' });
  await act(async () => { resolveOld([app]); });
  expect(screen.queryByRole('heading', { name: 'QQ' })).toBeNull();
});
const app = { id: '550e8400-e29b-41d4-a716-446655440000', platform: 'macos', display_name: 'QQ', platform_application_id: 'com.tencent.qq', launch_target: '/Applications/QQ.app', alternate_launch_targets: [], version: '1', discovery_source: 'automatic', is_present: true, last_discovered_at: '1', created_at: '1', updated_at: '1', accounts: [{ id: '550e8400-e29b-41d4-a716-446655440001', application_id: '550e8400-e29b-41d4-a716-446655440000', display_name: 'Personal', username: '123456', login_method: 'password', phone: null, auto_submit_enabled: false, last_login_status: null, last_login_at: null, created_at: '1', updated_at: '1' }] };
beforeEach(() => { invoke.mockReset(); invoke.mockImplementation(async name => name === 'list_applications' ? [app] : name === 'get_scan_status' ? { id: 0, phase: 'idle', started_at: null, count: null, error: null } : name === 'next_account_number' ? 2 : null); });
it('opens the app explicitly and exposes only credential actions', async () => {
  render(<ApplicationLibrary />); await screen.findByText('123456');
  expect(invoke.mock.calls.some(([name]) => name === 'launch_application')).toBe(false);
  await userEvent.click(screen.getByRole('button', { name: 'Open QQ' }));
  expect(invoke).toHaveBeenCalledWith('launch_application', { id: app.id });
  await userEvent.click(screen.getByRole('button', { name: 'Copy password · Personal' }));
  expect(invoke).toHaveBeenCalledWith('copy_application_password', { id: app.accounts[0].id });
  expect(screen.queryByText('Sign-in method')).toBeNull();
  expect(screen.queryByText('Fill')).toBeNull();
});
it('keeps scan and technical information in management', async () => {
  render(<ApplicationLibrary />); await screen.findByText('QQ');
  expect(screen.queryByText('Scan apps')).toBeNull();
  expect(screen.queryByText(app.launch_target)).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: 'Manage applications' }));
  expect(screen.getByRole('button', { name: 'Scan apps' })).toBeVisible();
  await userEvent.click(screen.getByRole('button', { name: 'Application actions · QQ' }));
  await userEvent.click(screen.getByRole('button', { name: 'Application information' }));
  expect(screen.getByText(app.launch_target)).toBeVisible();
});
it('saves only password accounts and leaves default names to the backend', async () => {
  invoke.mockImplementation(async (name, args) => name === 'list_applications' ? [app] : name === 'get_scan_status' ? { id: 0, phase: 'idle', started_at: null, count: null, error: null } : name === 'next_account_number' ? 2 : name === 'save_application_account' ? { ...app.accounts[0], ...args.request, display_name: 'Account 2' } : null);
  render(<ApplicationLibrary />); await screen.findByText('QQ');
  await userEvent.click(screen.getByRole('button', { name: 'Add account · QQ' }));
  expect(await screen.findByPlaceholderText('Account 2')).toHaveValue('');
  await userEvent.type(screen.getByLabelText('Login account'), '789');
  await userEvent.type(screen.getByLabelText('Password'), 'fixture');
  await userEvent.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('save_application_account', { request: expect.objectContaining({ display_name: '', username: '789', login_method: 'password', phone: null, auto_submit_enabled: false }) }));
});
