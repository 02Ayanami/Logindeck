import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, expect, it, vi } from 'vitest';
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
import '../../app/i18n';
import { WebsiteList } from './WebsiteList';

it('ignores older list responses and errors after a newer refresh', async () => {
  render(<WebsiteList />);
  await screen.findByText('login0');
  let resolveOld!: (value: unknown) => void;
  invoke.mockImplementationOnce(() => new Promise(resolve => { resolveOld = resolve; }));
  await act(async () => { window.dispatchEvent(new Event('focus')); });
  invoke.mockImplementationOnce(async () => []);
  await act(async () => { window.dispatchEvent(new Event('focus')); });
  expect(screen.queryByText('login0')).toBeNull();
  await act(async () => { resolveOld(records); });
  expect(screen.queryByText('login0')).toBeNull();
  let rejectOld!: (error: unknown) => void;
  invoke.mockImplementationOnce(() => new Promise((_, reject) => { rejectOld = reject; }));
  await act(async () => { window.dispatchEvent(new Event('focus')); });
  invoke.mockImplementationOnce(async () => records);
  await act(async () => { window.dispatchEvent(new Event('focus')); });
  await act(async () => { rejectOld({ code: 'storage.database', params: {} }); });
  expect(screen.getByText('login0')).toBeVisible();
  expect(screen.queryByRole('alert')).toBeNull();
});

it('reports every changed account and renews feedback only for the account updated again', async () => {
  render(<WebsiteList />);
  await screen.findByText('login0');
  vi.useFakeTimers();
  try {
    let current = records.map(record => ({ ...record, capture_source: 'browser_extension', updated_at: '2' }));
    invoke.mockImplementation(async name => name === 'list_websites' ? current : null);
    await act(async () => { window.dispatchEvent(new Event('focus')); });
    expect(screen.getAllByRole('status')).toHaveLength(4);
    expect(screen.getByText('login3')).toBeVisible();
    for (const record of records) {
      expect(screen.getByText(`GitHub · Updated ${record.account_name} password`)).toBeVisible();
      expect(screen.getByText(record.username).closest('.credential-row')).toHaveAttribute('data-highlight', 'true');
    }
    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
    current = current.map((record, index) => index === 0 ? { ...record, updated_at: '3' } : record);
    await act(async () => { window.dispatchEvent(new Event('focus')); });
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(screen.getAllByRole('status')).toHaveLength(1);
    expect(screen.getByText('GitHub · Updated Account 1 password')).toBeVisible();
    expect(screen.getByText('login0').closest('.credential-row')).toHaveAttribute('data-highlight', 'true');
    expect(screen.getByText('login1').closest('.credential-row')).toHaveAttribute('data-highlight', 'false');
    await act(async () => { await vi.advanceTimersByTimeAsync(2999); });
    expect(screen.getByRole('status')).toBeVisible();
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(screen.queryByRole('status')).toBeNull();
    expect(screen.getByText('login0').closest('.credential-row')).toHaveAttribute('data-highlight', 'false');
  } finally { vi.useRealTimers(); }
});
const records = Array.from({ length: 4 }, (_, i) => ({ id: '550e8400-e29b-41d4-a716-44665544000' + i, name: 'GitHub', account_name: 'Account ' + (i + 1), url: 'https://github.com/login', normalized_origin: 'https://github.com', username: 'login' + i, notes: '', capture_source: 'manual', created_at: '1', updated_at: '1' }));
beforeEach(() => { invoke.mockReset(); invoke.mockImplementation(async (name, args) => {
  if (name === 'list_websites') return records;
  if (name === 'next_account_number') return 5;
  if (name === 'copy_website_username') return 'validated-identifier';
  if (name === 'save_website') return { ...records[0], ...args.request, account_name: args.request.account_name || 'Account 5' };
  return null;
}); });
it('groups accounts, shows three, expands and searches real login identifiers', async () => {
  render(<WebsiteList />);
  expect(await screen.findByRole('heading', { name: 'GitHub' })).toBeVisible();
  expect(screen.queryByText('login3')).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: 'Show remaining 1 accounts' }));
  expect(screen.getByText('login3')).toBeVisible();
  await userEvent.type(screen.getByRole('searchbox'), 'login3');
  expect(screen.queryByText('login0')).toBeNull();
  expect(screen.getByText('login3')).toBeVisible();
});
it('copies through backend validation and never exposes a password response', async () => {
  const user = userEvent.setup();
  render(<WebsiteList />); await screen.findByText('login0');
  const clipboard = vi.spyOn(navigator.clipboard, 'writeText');
  await user.click(screen.getByRole('button', { name: 'Copy account · Account 1' }));
  expect(clipboard).toHaveBeenCalledWith('validated-identifier');
  await user.click(screen.getByRole('button', { name: 'Copy password · Account 1' }));
  expect(invoke).toHaveBeenCalledWith('copy_website_password', { id: records[0].id });
  expect(clipboard).toHaveBeenCalledTimes(1);
});
it('uses a placeholder name and preserves the stored password on metadata edits', async () => {
  render(<WebsiteList />); await screen.findByText('login0');
  await userEvent.click(screen.getByRole('button', { name: 'Add account · GitHub' }));
  expect(await screen.findByPlaceholderText('Account 5')).toHaveValue('');
  await userEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  await userEvent.click(screen.getByRole('button', { name: 'Account actions · Account 1' }));
  await userEvent.click(screen.getByRole('button', { name: 'Edit account' }));
  await userEvent.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('save_website', { request: expect.objectContaining({ id: records[0].id, password: undefined, account_name: 'Account 1', username: 'login0' }) }));
});
it('keeps an account when secure deletion fails and has no bulk deletion action', async () => {
  invoke.mockImplementation(async name => name === 'list_websites' ? [records[0]] : name === 'delete_website' ? Promise.reject({ code: 'credential.denied', params: {} }) : 2);
  render(<WebsiteList />); await screen.findByText('login0');
  await userEvent.click(screen.getByRole('button', { name: 'Account actions · Account 1' }));
  await userEvent.click(screen.getByRole('button', { name: 'Delete account' }));
  expect(screen.getByText(/last account/)).toBeVisible();
  await userEvent.click(screen.getByRole('button', { name: 'Delete account' }));
  await waitFor(() => expect(screen.getAllByRole('alert').length).toBeGreaterThan(0));
  expect(screen.getByRole('dialog', { name: 'Delete account' })).toBeVisible();
  expect(screen.getByText('login0')).toBeVisible();
  expect(screen.queryByText('Delete website')).toBeNull();
});
