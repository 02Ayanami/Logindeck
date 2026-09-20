import { useEffect, useState } from 'react';
import { Drawer, useWords } from '../../components/ui/VaultWorkspace';
import { tauri, TauriCommandError } from '../../lib/tauri';
import { translateCommandError } from '../../app/i18n';

export type AccountDraft = { name: string; username: string; password: string; websiteName: string; url: string };
export function AccountDrawer({ title, target, scope, initial, websiteFields = false, onSave, onClose, onDelete }: {
  title: string; target?: string; scope?: string; initial?: Partial<AccountDraft>; websiteFields?: boolean;
  onSave: (draft: AccountDraft) => Promise<void>; onClose: () => void; onDelete?: () => void;
}) {
  const w = useWords();
  const [draft, setDraft] = useState<AccountDraft>({ name: '', username: '', password: '', websiteName: '', url: '', ...initial });
  const [number, setNumber] = useState<number>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  let effectiveScope = scope;
  if (!effectiveScope && draft.url) { try { effectiveScope = 'website:' + new URL(draft.url).origin; } catch { /* Incomplete URL while typing. */ } }
  useEffect(() => { let live = true; if (initial?.name) { setNumber(1); return; } setNumber(undefined); if (!effectiveScope) { setNumber(1); return; } void tauri.nextAccountNumber(effectiveScope).then(n => { if (live) setNumber(n); }).catch(() => { if (live) setError(w('无法读取默认名称，请重新打开', 'Could not load the default name. Please reopen.')); }); return () => { live = false; }; }, [effectiveScope]);
  const field = (key: keyof AccountDraft, label: string, type = 'text', required = true) => <label>{label}<input type={type} value={draft[key]} required={required} disabled={busy} maxLength={key === 'password' ? 16384 : key === 'url' ? 2048 : key === 'username' ? 512 : 256} placeholder={key === 'name' ? initial?.name || (number ? w('账号 ', 'Account ') + number : '') : undefined} autoComplete={key === 'password' ? 'new-password' : 'off'} onChange={event => setDraft({ ...draft, [key]: event.target.value })} /></label>;
  return <Drawer title={title} subtitle={target} onClose={onClose} busy={busy}><form className="workspace-form" onSubmit={async event => {
    event.preventDefault(); if (busy) return; setBusy(true); setError(undefined);
    try { await onSave(draft); onClose(); } catch (reason) { setError(translateCommandError(reason instanceof TauriCommandError ? reason.code : 'internal.error', {})); }
    finally { setDraft(current => ({ ...current, password: '' })); setBusy(false); }
  }}>
    {websiteFields && <><div className="form-section">{w('网站信息', 'Website')}</div>{field('websiteName', w('网站名称', 'Website name'))}{field('url', w('网站地址', 'Website URL'), 'url')}<div className="form-section">{w('首个账号', 'First account')}</div></>}
    {field('name', w('账号名称', 'Account name'), 'text', false)}
    {field('username', w('登录账号', 'Login account'))}
    {field('password', initial?.username ? w('新密码', 'New password') : w('密码', 'Password'), 'password', !initial?.username)}
    {initial?.username && <p className="form-hint">{w('留空将保留当前密码。', 'Leave blank to keep the current password.')}</p>}
    {error && <p role="alert">{error}</p>}
    <footer className="workspace-form__footer">{onDelete && <button type="button" className="text-danger" disabled={busy} onClick={onDelete}>{w('删除账号', 'Delete account')}</button>}<span /><button type="button" className="secondary-button" disabled={busy} onClick={onClose}>{w('取消', 'Cancel')}</button><button disabled={busy || number === undefined}>{busy ? w('正在保存…', 'Saving…') : w('保存', 'Save')}</button></footer>
  </form></Drawer>;
}
