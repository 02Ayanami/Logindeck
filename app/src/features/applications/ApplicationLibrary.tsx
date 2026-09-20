import { useEffect, useRef, useState } from 'react';
import { tauri, TauriCommandError, type Application, type ApplicationAccount } from '../../lib/tauri';
import { translateCommandError } from '../../app/i18n';
import { AccountRow, Drawer, Icon, IconButton, MoreMenu, WorkspaceHeader, useWords } from '../../components/ui/VaultWorkspace';
import { AccountDrawer, type AccountDraft } from '../websites/AccountDrawer';
import { ApplicationIcon } from './ApplicationIcon';
import { ApplicationScanDialog } from './ApplicationScanDialog';
import { useApplicationScan } from './useApplicationScan';
export function ApplicationLibrary() {
  const w = useWords();
  const [apps, setApps] = useState<Application[]>([]);
  const [loading, setLoading] = useState(true);
  const [manage, setManage] = useState(false);
  const [search, setSearch] = useState('');
  const [expanded, setExpanded] = useState<string[]>([]);
  const [editor, setEditor] = useState<{ app: Application; account?: ApplicationAccount }>();
  const [removing, setRemoving] = useState<{ app: Application; account?: ApplicationAccount }>();
  const [info, setInfo] = useState<Application>();
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [busy, setBusy] = useState(false);
  const [highlight, setHighlight] = useState<string>();
  const flight = useRef(false);
  const loadVersion = useRef(0);
  const showError = (reason: unknown) => setError(translateCommandError(reason instanceof TauriCommandError ? reason.code : 'internal.error', {}));
  async function load() {
    const version = ++loadVersion.current;
    try {
      const result = await tauri.listApplications();
      if (version === loadVersion.current) setApps(result);
    } catch (reason) { if (version === loadVersion.current) showError(reason); }
    finally { if (version === loadVersion.current) setLoading(false); }
  }
  useEffect(() => { void load(); return () => { loadVersion.current++; }; }, []);
  const scan = useApplicationScan(load);
  const blocked = busy || scan.active || scan.checking || loading;
  useEffect(() => { if (!highlight) return; const timer = setTimeout(() => setHighlight(undefined), 4000); return () => clearTimeout(timer); }, [highlight]);
  useEffect(() => { if (!notice) return; const timer = setTimeout(() => setNotice(''), 4000); return () => clearTimeout(timer); }, [notice]);
  async function run(operation: () => Promise<void>) {
    if (flight.current) return; flight.current = true; setBusy(true); setError('');
    loadVersion.current++;
    try { await operation(); } catch (reason) { showError(reason); } finally { flight.current = false; setBusy(false); }
  }
  async function save(draft: AccountDraft) {
    if (!editor) return;
    loadVersion.current++;
    const saved = await tauri.saveApplicationAccount({ id: editor.account?.id, applicationId: editor.app.id, displayName: draft.name.trim(), username: draft.username.trim(), password: draft.password || undefined, autoSubmitEnabled: false, loginMethod: 'password', phone: null });
    await load(); setExpanded(current => [...current, editor.app.id]); setHighlight(saved.id); setNotice(editor.app.displayName + ' · ' + w('已保存 ', 'Saved ') + saved.displayName);
  }
  async function copy(account: ApplicationAccount, password: boolean) {
    setError('');
    try { if (password) await tauri.copyApplicationPassword(account.id); else await navigator.clipboard.writeText(await tauri.copyApplicationUsername(account.id)); }
    catch (reason) { if (reason instanceof TauriCommandError && reason.code === 'credential.cancelled') setNotice(w('已取消复制密码', 'Password copy cancelled')); else showError(reason); throw reason; }
  }
  const query = search.trim().toLowerCase();
  const filtered = apps.filter(app => app.displayName.toLowerCase().includes(query) || (!manage && app.accounts.some(account => (account.displayName + ' ' + account.username).toLowerCase().includes(query))));
  return <section className="workspace-page">
    {manage && <button className="workspace-back" onClick={() => { setManage(false); setSearch(''); }}><Icon name="arrow" />{w('返回应用账号', 'Back to accounts')}</button>}
    <WorkspaceHeader title={manage ? w('管理应用', 'Manage applications') : w('应用账号', 'Application accounts')} description={manage ? w('选择出现在账号库中的桌面应用。', 'Choose the desktop apps in your vault.') : w('熟悉的应用，随手可用的账号。', 'Your everyday apps. Your accounts, within reach.')} search={search} setSearch={setSearch} actions={manage ? <><button className="secondary-button" disabled={blocked} onClick={() => void run(async () => { const path = await tauri.pickApplicationBundle(); if (path) { await tauri.importApplications([path]); await load(); } })}>{w('手动添加', 'Add manually')}</button><button disabled={blocked} onClick={() => void scan.start()}><Icon name="plus" />{w('扫描添加', 'Scan apps')}</button></> : <button className="secondary-button" onClick={() => { setManage(true); setSearch(''); }}>{w('管理应用', 'Manage applications')}</button>} />
    {error && <p role="alert" className="workspace-alert">{error}</p>}{notice && <p role="status" className="workspace-notice">{notice}</p>}
    {Boolean(scan.error) && <p role="alert">{translateCommandError(scan.error instanceof TauriCommandError ? scan.error.code : 'internal.error', {})}</p>}
    {scan.active && scan.status?.phase === 'scanning' && <div className="workspace-notice" role="status">{w('正在扫描应用…', 'Scanning applications…')} <button className="workspace-back" onClick={() => void scan.cancel()}>{w('取消', 'Cancel')}</button></div>}
    <div className="workspace-count">{w('已添加', 'Added')} · {apps.length} {w('个应用', 'applications')}</div>
    {loading ? <p className="workspace-empty">{w('正在加载…', 'Loading…')}</p> : filtered.length === 0 ? <div className="workspace-empty"><h2>{query ? w('没有匹配的应用', 'No matching applications') : w('添加你常用的应用', 'Add your everyday apps')}</h2><p>{w('通过管理应用扫描本机或手动添加。', 'Scan your computer or choose an application manually.')}</p></div> : manage ? <div className="application-table">{filtered.map(app => <div className="application-table__row" key={app.id}><ApplicationIcon application={app} revision={0} /><strong>{app.displayName}</strong><span>{app.accounts.length} {w('个账号', 'accounts')}</span><MoreMenu label={w('应用操作', 'Application actions') + ' · ' + app.displayName} items={[{ label: w('查看应用信息', 'Application information'), action: () => setInfo(app) }, { label: w('移除应用', 'Remove application'), danger: true, action: () => { setError(''); setRemoving({ app }); } }]} /></div>)}</div> : <div className="target-grid">{filtered.map(app => {
      const accounts = !query || app.displayName.toLowerCase().includes(query) ? app.accounts : app.accounts.filter(account => (account.displayName + ' ' + account.username).toLowerCase().includes(query));
      const all = Boolean(query) || expanded.includes(app.id);
      return <article className="target-card" key={app.id}><header className="target-card__header"><ApplicationIcon application={app} revision={0} /><div className="target-card__identity"><h2>{app.displayName}</h2><span>{app.accounts.length} {w('个账号', 'accounts')}</span></div><div className="target-card__actions"><IconButton icon="plus" label={w('添加账号', 'Add account') + ' · ' + app.displayName} onClick={() => setEditor({ app })} /><IconButton icon="open" label={w('打开', 'Open') + ' ' + app.displayName} disabled={busy} onClick={() => void run(() => tauri.launchApplication(app.id))} /></div></header>
      {(all ? accounts : accounts.slice(0, 3)).map(account => <AccountRow key={account.id} name={account.displayName} username={account.username} passwordAvailable={account.loginMethod === 'password'} highlight={highlight === account.id} onCopy={password => copy(account, password)} onEdit={() => setEditor({ app, account })} onDelete={() => { setError(''); setRemoving({ app, account }); }} />)}
      {!accounts.length && <div className="target-card__empty">{w('还没有账号，点击右上角 ＋ 添加。', 'No accounts yet. Use + above to add one.')}</div>}
      {accounts.length > 3 && <button className="target-card__expand" onClick={() => setExpanded(current => all ? current.filter(id => id !== app.id) : [...current, app.id])}>{all ? w('收起账号', 'Show less') : w('查看其余 ', 'Show remaining ') + (accounts.length - 3) + w(' 个账号', ' accounts')}</button>}
      </article>;
    })}</div>}
    {editor && <AccountDrawer title={editor.account ? w('编辑账号', 'Edit account') : w('添加账号', 'Add account')} target={editor.app.displayName} scope={'application:' + editor.app.id} initial={editor.account ? { name: editor.account.displayName, username: editor.account.username } : undefined} onSave={save} onClose={() => setEditor(undefined)} onDelete={editor.account ? () => setRemoving(editor) : undefined} />}
    {info && <Drawer title={w('应用信息', 'Application information')} subtitle={info.displayName} onClose={() => setInfo(undefined)}><dl className="detail-list"><div><dt>{w('版本', 'Version')}</dt><dd>{info.version ?? '—'}</dd></div><div><dt>{w('应用位置', 'Location')}</dt><dd>{info.launchTarget}</dd></div><div><dt>{w('应用标识符', 'Bundle ID')}</dt><dd>{info.platformApplicationId}</dd></div></dl></Drawer>}
    {removing && <Drawer title={removing.account ? w('删除账号', 'Delete account') : w('移除应用', 'Remove application')} subtitle={removing.app.displayName} onClose={() => setRemoving(undefined)} busy={busy}><p>{removing.account ? w('将删除此账号及其保存的密码。应用仍会保留。', 'Delete this account and its saved password. The application will remain.') : removing.app.accounts.length ? w('请先逐个删除该应用下的账号，再移除应用。', 'Delete each account before removing this application.') : w('将从账号库中移除此应用。', 'Remove this application from your vault.')}</p><p>{w('此操作无法撤销。', 'This cannot be undone.')}</p>{error && <p role="alert">{error}</p>}<footer className="workspace-form__footer"><button className="secondary-button" disabled={busy} onClick={() => setRemoving(undefined)}>{w('取消', 'Cancel')}</button><button className="danger" disabled={busy || (!removing.account && removing.app.accounts.length > 0)} onClick={() => void run(async () => { if (removing.account) await tauri.deleteApplicationAccount(removing.account.id); else await tauri.removeApplication(removing.app.id); await load(); setNotice(removing.app.displayName + ' · ' + w('已删除', 'Removed')); setRemoving(undefined); setEditor(undefined); })}>{removing.account ? w('删除账号', 'Delete account') : w('移除应用', 'Remove application')}</button></footer></Drawer>}
    {scan.status && ['choosing', 'committing'].includes(scan.status.phase) && <ApplicationScanDialog snapshot={scan.status} busy={scan.status.phase === 'committing'} onConfirm={tokens => void scan.confirm(tokens)} onCancel={() => void scan.cancel()} />}
  </section>;
}
