import { invoke } from '@tauri-apps/api/core';
import { z } from 'zod';

export const localeSchema = z.enum(['en', 'zh-CN']);
export type Locale = z.infer<typeof localeSchema>;
const uuid = z.uuid();
export const utf8Length = (value: string) => new TextEncoder().encode(value).length;
const boundedText = (max: number) => z.string().refine((value) => utf8Length(value) <= max);
const timestamp = z.string().regex(/^\d{1,16}$/);
const validationField = z.enum(['name', 'url', 'username', 'password', 'application_id', 'notes', 'display_name', 'platform_application_id', 'launch_target', 'version', 'signature_identity', 'path_access_ref', 'paths', 'authentication_reason', 'phone', 'login_method']);
const noParams = z.object({}).strict();
const commandErrorSchema = z.union([
  z.object({ code: z.literal('validation.invalid_field'), params: z.object({ field: validationField }).strict() }).strict(),
  ...['credential.denied', 'credential.cancelled', 'credential.not_found', 'credential.missing_entitlement', 'application.scan_timeout', 'application.cancelled', 'credential.unavailable', 'credential.cleanup_required', 'credential.compensation_failed', 'credential.compensation_tracking_failed', 'credential.cleanup_tracking_failed', 'clipboard.unavailable', 'application.not_found', 'application.signature_changed', 'application.launch_failed', 'application.unsupported_target', 'application.unsupported_import', 'application.discovery_unavailable', 'storage.conflict', 'storage.referential_integrity', 'storage.not_found', 'storage.database', 'platform.unsupported', 'extension.setup_failed', 'extension.setup_conflict', 'extension.resources_missing', 'extension.edge_unavailable', 'internal.error'].map((code) => z.object({ code: z.literal(code), params: noParams }).strict()),
]);
export type CommandError = z.infer<typeof commandErrorSchema>;

export class TauriCommandError extends Error {
  readonly code: CommandError['code'];
  readonly params: CommandError['params'];
  constructor(error: CommandError) { super(error.code); this.name = 'TauriCommandError'; this.code = error.code; this.params = error.params; }
}
const internalError: CommandError = { code: 'internal.error', params: {} };
const safeError = (value: unknown): CommandError => commandErrorSchema.safeParse(value).data ?? internalError;

export function parseSafeWebsiteUrl(value: string): string | undefined {
  try {
    const url = new URL(value);
    return (url.protocol === 'http:' || url.protocol === 'https:') && !url.username && !url.password && utf8Length(url.href) <= 2048 ? url.href : undefined;
  } catch { return undefined; }
}
const safeUrl = z.string().max(2048).refine((value) => parseSafeWebsiteUrl(value) !== undefined);
const websiteWireSchema = z.object({
  account_name: boundedText(256).default(''),
  id: uuid, name: boundedText(256), url: safeUrl, normalized_origin: safeUrl, username: boundedText(512), notes: boundedText(4000),
  capture_source: z.enum(['manual', 'browser_extension']), created_at: timestamp, updated_at: timestamp,
}).strict().transform((record) => ({ accountName: record.account_name, id: record.id, name: record.name, url: record.url, normalizedOrigin: record.normalized_origin, username: record.username, notes: record.notes, captureSource: record.capture_source, createdAt: record.created_at, updatedAt: record.updated_at }));
export type Website = z.infer<typeof websiteWireSchema>;
const settingsSchema = z.object({ ui_locale: localeSchema }).strict().transform(({ ui_locale }) => ({ uiLocale: ui_locale }));
export type SaveWebsiteRequest = { id?: string; name: string; url: string; username: string; password?: string; notes: string; account_name?: string };
export const loginMethodSchema = z.literal('password');
export type LoginMethod = z.infer<typeof loginMethodSchema>;
const accountWireSchema = z.object({ id: uuid, application_id: uuid, display_name: boundedText(256), username: boundedText(512), login_method: z.enum(['password', 'sms', 'qr']).default('password'), phone: boundedText(16).nullable().default(null), auto_submit_enabled: z.boolean(), last_login_status: z.string().max(256).nullable(), last_login_at: timestamp.nullable(), created_at: timestamp, updated_at: timestamp }).strict().transform((x) => ({ id: x.id, applicationId: x.application_id, displayName: x.display_name, username: x.username, loginMethod: x.login_method, phone: x.phone, autoSubmitEnabled: x.auto_submit_enabled, lastLoginStatus: x.last_login_status, lastLoginAt: x.last_login_at, createdAt: x.created_at, updatedAt: x.updated_at }));
export type ApplicationAccount = z.infer<typeof accountWireSchema>;
const pathSchema = boundedText(4096).refine((value) => value.trim().length > 0);
const applicationWireSchema = z.object({
  id: uuid, platform: z.enum(['macos', 'windows']), display_name: boundedText(256), platform_application_id: boundedText(512), launch_target: pathSchema,
  alternate_launch_targets: z.array(pathSchema).max(32), version: boundedText(256).nullable(), discovery_source: z.enum(['automatic', 'manual_import']),
  is_present: z.boolean(), last_discovered_at: timestamp, created_at: timestamp, updated_at: timestamp, accounts: z.array(accountWireSchema),
}).strict().transform((x) => ({ id: x.id, platform: x.platform, displayName: x.display_name, platformApplicationId: x.platform_application_id, launchTarget: x.launch_target, alternateLaunchTargets: x.alternate_launch_targets, version: x.version, discoverySource: x.discovery_source, isPresent: x.is_present, lastDiscoveredAt: x.last_discovered_at, createdAt: x.created_at, updatedAt: x.updated_at, accounts: x.accounts }));
export type Application = z.infer<typeof applicationWireSchema>;
export type SaveApplicationAccountRequest = { id?: string; applicationId: string; displayName: string; username: string; loginMethod?: LoginMethod; phone?: string | null; password?: string; autoSubmitEnabled: boolean };
const unit = z.union([z.undefined(), z.null()]).transform(() => undefined);
async function command<T>(name: string, schema: z.ZodType<T>, payload?: Record<string, unknown>): Promise<T> {
  try { const value: unknown = payload === undefined ? await invoke(name) : await invoke(name, payload); const parsed = schema.safeParse(value); if (!parsed.success) throw new TauriCommandError(internalError); return parsed.data; }
  catch (error) { if (error instanceof TauriCommandError) throw error; throw new TauriCommandError(safeError(error)); }
}
export async function openUrl(value: string): Promise<void> {
  const url = parseSafeWebsiteUrl(value); if (!url) throw new TauriCommandError({ code: 'validation.invalid_field', params: { field: 'url' } });
  await command('plugin:opener|open_url', unit, { url });
}
const dialogResultSchema = z.union([z.null(), pathSchema, z.array(pathSchema).max(32)]);
async function pickPath(options: { directory: boolean; filters?: { name: string; extensions: string[] }[] }): Promise<string | undefined> {
  // The dialog plugin is registered in Rust. Keep this tiny direct wire rather than importing a
  // missing JS package, and never provide a general filesystem capability.
  const value = await command('plugin:dialog|open', dialogResultSchema, { options: { multiple: false, directory: options.directory, filters: options.filters } });
  return Array.isArray(value) ? value[0] : value ?? undefined;
}
const scanCandidateSchema = z.object({ token: z.number().int().nonnegative(), display_name: boundedText(256), selected: z.boolean() }).strict();
export type ScanCandidate = z.infer<typeof scanCandidateSchema>;
const scanSchema = z.object({ id: z.number().int().nonnegative(), phase: z.enum(['idle', 'scanning', 'choosing', 'cancelling', 'committing', 'completed', 'cancelled', 'failed']), started_at: z.number().nullable(), count: z.number().int().nonnegative().nullable(), candidates: z.array(scanCandidateSchema).max(4096).default([]), error: commandErrorSchema.nullable() }).strict();
export type ScanStatus = z.infer<typeof scanSchema>;
const maintenanceSchema = z.object({ pending_count: z.number().int().nonnegative(), storage_backend: z.enum(['login_keychain', 'windows_credential_manager']), last_os_status: z.number().int().nullable(), last_error: commandErrorSchema.nullable() }).strict();
export type CredentialMaintenance = z.infer<typeof maintenanceSchema>;
// Matches the bounded Windows icon facade; macOS emits a smaller PNG within this same contract.
const iconSchema = z.string().max(1500 * 1024).regex(/^data:image\/png;base64,iVBORw0KGgo[A-Za-z0-9+/]*={0,2}$/).nullable();
const browserCaptureSchema = z.object({ enabled: z.boolean(), revision: z.number().int(), last_connected_at: z.number().int().nullable() }).strict();
export type BrowserCaptureSettings = z.infer<typeof browserCaptureSchema>;
export const tauri = {
  nextAccountNumber: (scope: string) => command('next_account_number', z.number().int().positive(), { scope }),
  editWebsiteGroup: (origin: string, name: string, url: string) => command('edit_website_group', unit, { origin, name, url }),
  removeApplication: (id: string) => command('remove_application', unit, { id }),
  installEdgeExtension: () => command('install_edge_extension', z.object({ extension_path: z.string().min(1) }).strict()),
  openEdgeExtensions: () => command('open_edge_extensions', unit),
  openEdgeExtensionFolder: () => command('open_edge_extension_folder', unit),
  getBrowserCaptureSettings: () => command('get_browser_capture_settings', browserCaptureSchema),
  setBrowserCaptureEnabled: (enabled: boolean) => command('set_browser_capture_enabled', browserCaptureSchema, { enabled }),
  listWebsites: () => command('list_websites', z.array(websiteWireSchema)), saveWebsite: (request: SaveWebsiteRequest) => command('save_website', websiteWireSchema, { request }), deleteWebsite: (id: string) => command('delete_website', unit, { id }),
  copyWebsiteUsername: (id: string) => command('copy_website_username', boundedText(512).min(1), { id }), copyWebsitePassword: (id: string) => command('copy_website_password', unit, { id }),
  getSettings: () => command('get_settings', settingsSchema), setLocale: (locale: Locale) => command('set_locale', settingsSchema, { locale }),
  getApplicationIcon: (id: string) => command('get_application_icon', iconSchema, { id }),
  getScanCandidateIcon: (id: number, token: number) => command('get_scan_candidate_icon', iconSchema, { id, token }),
  listApplications: () => command('list_applications', z.array(applicationWireSchema)),
  rescanApplications: () => command('rescan_applications', scanSchema),
  getScanStatus: () => command('get_scan_status', scanSchema),
  confirmApplicationScan: (id: number, tokens: number[]) => command('confirm_application_scan', scanSchema, { id, tokens }),
  cancelApplicationScan: (id: number) => command('cancel_application_scan', scanSchema, { id }),
  getCredentialMaintenance: () => command('get_credential_maintenance', maintenanceSchema),
  retryCredentialCleanup: () => command('retry_credential_cleanup', maintenanceSchema),
  importApplications: (paths: string[]) => command('import_applications', z.array(applicationWireSchema), { paths }),
  saveApplicationAccount: (request: SaveApplicationAccountRequest) => command('save_application_account', accountWireSchema, { request: { id: request.id, application_id: request.applicationId, display_name: request.displayName, username: request.username, password: request.password, login_method: request.loginMethod ?? 'password', phone: request.phone ?? null, auto_submit_enabled: request.autoSubmitEnabled } }),
  deleteApplicationAccount: (id: string) => command('delete_application_account', unit, { id }),
  copyApplicationUsername: (id: string) => command('copy_application_username', boundedText(512).min(1), { id }),
  copyApplicationPassword: (id: string) => command('copy_application_password', unit, { id }),
  launchApplication: (id: string) => command('launch_application', unit, { id }),
  pickApplicationBundle: () => pickPath({ directory: false, filters: [{ name: 'Application', extensions: ['app', 'exe'] }] }),
};
