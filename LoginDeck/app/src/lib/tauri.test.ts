import { beforeEach, describe, expect, it, vi } from 'vitest';
const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
import { openUrl, tauri, TauriCommandError } from './tauri';

const record = { id: '550e8400-e29b-41d4-a716-446655440000', name: 'Example', url: 'https://example.com', normalized_origin: 'https://example.com', username: 'alice', notes: '', capture_source: 'manual', created_at: '1726099200', updated_at: '1726099201' };
describe('Tauri boundary', () => {
  beforeEach(() => invokeMock.mockReset());
  it('only accepts strict snake_case safe website DTOs', async () => {
    invokeMock.mockResolvedValue([record]);
    await expect(tauri.listWebsites()).resolves.toHaveLength(1);
    for (const unsafe of [{ ...record, password: 'x' }, { ...record, password_credential_ref: 'opaque' }, { ...record, normalizedOrigin: record.normalized_origin }, { ...record, url: 'https://alice:pw@example.com' }, { ...record, id: 'not-a-uuid' }, { ...record, name: 'x'.repeat(257) }]) {
      invokeMock.mockResolvedValueOnce([unsafe]);
      await expect(tauri.listWebsites()).rejects.toMatchObject({ code: 'internal.error', params: {} });
    }
  });
  it('allows only exact command error codes and params', async () => {
    invokeMock.mockRejectedValueOnce({ code: 'validation.invalid_field', params: { field: 'url' } });
    await expect(tauri.listWebsites()).rejects.toMatchObject({ code: 'validation.invalid_field', params: { field: 'url' } });
    invokeMock.mockRejectedValueOnce({ code: 'unknown', params: { value: 'x'.repeat(999) } });
    await expect(tauri.listWebsites()).rejects.toMatchObject({ code: 'internal.error', params: {} });
  });
  it('opens only safe http(s) URLs through the opener plugin', async () => {
    invokeMock.mockResolvedValue(undefined);
    await openUrl('https://example.com/path');
    expect(invokeMock).toHaveBeenCalledWith('plugin:opener|open_url', { url: 'https://example.com/path' });
    await expect(openUrl('javascript:alert(1)')).rejects.toBeInstanceOf(TauriCommandError);
    await expect(openUrl('https://alice:pw@example.com')).rejects.toBeInstanceOf(TauriCommandError);
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

});

const application = { id: '550e8400-e29b-41d4-a716-446655440000', platform: 'macos', display_name: 'Example', platform_application_id: 'com.example.app', launch_target: '/Applications/Example.app', alternate_launch_targets: ['/Users/a/Applications/Example.app'], version: null, discovery_source: 'automatic', is_present: true, last_discovered_at: '1726099200', created_at: '1726099200', updated_at: '1726099200', accounts: [] };
it('accepts only strict safe application DTOs and never signature or secret fields', async () => {
  invokeMock.mockResolvedValue([application]); await expect(tauri.listApplications()).resolves.toHaveLength(1);
  invokeMock.mockResolvedValueOnce([{ ...application, launch_target: '' }]); await expect(tauri.listApplications()).rejects.toMatchObject({ code: 'internal.error' });
  invokeMock.mockResolvedValueOnce([{ ...application, launch_target: '界'.repeat(1366) }]); await expect(tauri.listApplications()).rejects.toMatchObject({ code: 'internal.error' });
  invokeMock.mockResolvedValueOnce([{ ...application, signature_identity: 'private' }]); await expect(tauri.listApplications()).rejects.toMatchObject({ code: 'internal.error' });
  invokeMock.mockResolvedValueOnce([{ ...application, accounts: [{ id: application.id, application_id: application.id, display_name: 'a', username: 'u', password_credential_ref: 'opaque', auto_submit_enabled: false, last_login_status: null, last_login_at: null, created_at: '1726099200', updated_at: '1726099200' }] }]); await expect(tauri.listApplications()).rejects.toMatchObject({ code: 'internal.error' });
});
it('uses the registered dialog wire and treats cancellation as a non-error', async () => {
  invokeMock.mockResolvedValueOnce(null); await expect(tauri.pickApplicationBundle()).resolves.toBeUndefined(); expect(invokeMock).toHaveBeenCalledWith('plugin:dialog|open', { options: { multiple: false, directory: false, filters: [{ name: 'Application', extensions: ['app'] }] } });
});

it('grants only scoped HTTP(S) opening, without filesystem or shell permissions', async () => {
 const capability = (await import('../../src-tauri/capabilities/default.json')).default;
 expect(capability.permissions).toContainEqual({ identifier: 'opener:allow-open-url', allow: [{ url: 'http://*' }, { url: 'https://*' }] });
 expect(capability.permissions).toHaveLength(3);
 expect(capability.permissions).toContain('core:default');
 expect(capability.permissions).toContain('dialog:allow-open');
});

it.each(['platform_application_id', 'launch_target', 'version', 'signature_identity', 'path_access_ref'])('preserves the safe %s validation field from Rust', async (field) => {
 invokeMock.mockRejectedValueOnce({ code: 'validation.invalid_field', params: { field } });
 await expect(tauri.listApplications()).rejects.toMatchObject({ code: 'validation.invalid_field', params: { field } });
});

it('accepts only bounded PNG icon data from the narrow application ID command', async () => {
 invokeMock.mockReset();
 invokeMock.mockResolvedValueOnce('data:image/png;base64,iVBORw0KGgo=');
 await expect(tauri.getApplicationIcon(application.id)).resolves.toMatch(/^data:image\/png/);
 expect(invokeMock).toHaveBeenCalledWith('get_application_icon', { id: application.id });
 for (const value of ['https://example.com/icon.png', 'data:image/svg+xml,<svg/>', 'data:image/png;base64,' + 'A'.repeat(90000)]) {
  invokeMock.mockResolvedValueOnce(value);
  await expect(tauri.getApplicationIcon(application.id)).rejects.toMatchObject({ code: 'internal.error' });
 }
 invokeMock.mockResolvedValueOnce(null);
 await expect(tauri.getApplicationIcon(application.id)).resolves.toBeNull();
});
it('reads scan icons only through a task id and candidate token', async () => {
 invokeMock.mockResolvedValueOnce('data:image/png;base64,iVBORw0KGgo=');
 await expect(tauri.getScanCandidateIcon(7, 2)).resolves.toMatch(/^data:image\/png/);
 expect(invokeMock).toHaveBeenCalledWith('get_scan_candidate_icon', { id: 7, token: 2 });
});

it('accepts the macOS login keychain credential backend', async () => {
 invokeMock.mockResolvedValueOnce({ pending_count: 0, storage_backend: 'login_keychain', last_os_status: null, last_error: null });
 await expect(tauri.getCredentialMaintenance()).resolves.toMatchObject({ storage_backend: 'login_keychain' });
});
