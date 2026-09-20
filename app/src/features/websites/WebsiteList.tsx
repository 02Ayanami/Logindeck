import { useEffect, useRef, useState } from 'react';
import { tauri, openUrl, TauriCommandError, type Website } from '../../lib/tauri';
import { translateCommandError } from '../../app/i18n';
import { AccountRow, Drawer, Icon, IconButton, MoreMenu, WorkspaceHeader, useWords } from '../../components/ui/VaultWorkspace';
import { AccountDrawer, type AccountDraft } from './AccountDrawer';
import { WebsiteIcon } from './WebsiteIcon';
import { PluginConnectionStatus } from '../settings/PluginConnectionStatus';
import { PluginGuide } from '../settings/PluginGuide';
type Group = { origin: string; name: string; url: string; accounts: Website[] };
export function WebsiteList() {
  const w = useWords();
  const [records, setRecords] = useState<Website[]>([]);
  const [loading, setLoading] = useState(true);
  const [query, setQuery] = useState('');
  const [notice, setNotice] = useState('');
  const [error, setError] = useState('');
  const [expanded, setExpanded] = useState<string[]>([]);
  const [editor, setEditor] = useState<{ group?: Group; account?: Website }>();
  const [editingGroup, setEditingGroup] = useState<Group>();
  const [deleting, setDeleting] = useState<Website>();
  const [plugin, setPlugin] = useState(false);
  const [busy, setBusy] = useState(false);
  const [highlight, setHighlight] = useState<string>();
  const [captureFeedback, setCaptureFeedback] = useState<Record<string, { text: string; expires: number }>>({});
  const flight = useRef(false);
  const loadVersion = useRef(0);
  const previous = useRef<Website[] | undefined>(undefined);
  const showError = (reason: unknown) => setError(translateCommandError(reason instanceof TauriCommandError ? reason.code : 'internal.error', {}));
  async function load() {
    const version = ++loadVersion.current;
    try {
      const result = await tauri.listWebsites();
      if (version !== loadVersion.current) return;
      if (previous.current) {
        const changed = result.filter(account => account.captureSource === 'browser_extension' && !previous.current!.some(old => old.id === account.id && old.updatedAt === account.updatedAt));
        if (changed.length) {
          const expires = Date.now() + 4000;
          const updates = Object.fromEntries(changed.map(account => {
            const exists = previous.current!.some(old => old.id === account.id);
            return [account.id, { expires, text: account.name + ' · ' + (exists ? w('已更新', 'Updated ') + account.accountName + w('的密码', ' password') : w('已新增', 'Added ') + account.accountName) }];
          }));
          setCaptureFeedback(current => ({ ...current, ...updates }));
          setExpanded(current => [...new Set([...current, ...changed.map(account => account.normalizedOrigin)])]);
        }
      }
      previous.current = result;
      setRecords(result);
    } catch (reason) { if (version === loadVersion.current) showError(reason); }
    finally { if (version === loadVersion.current) setLoading(false); }
  }
  useEffect(() => { void load(); return () => { loadVersion.current++; }; }, []);
  useEffect(() => { const refresh = () => { if (!editor && !editingGroup && !deleting && !flight.current) void load(); }; window.addEventListener('focus', refresh); const timer = setInterval(refresh, 5000); return () => { clearInterval(timer); window.removeEventListener('focus', refresh); }; }, [editor, editingGroup, deleting]);
  useEffect(() => { if (!highlight) return; const timer = setTimeout(() => setHighlight(undefined), 4000); return () => clearTimeout(timer); }, [highlight]);
  useEffect(() => { if (!notice) return; const timer = setTimeout(() => setNotice(''), 4000); return () => clearTimeout(timer); }, [notice]);
  useEffect(() => {
    const entries = Object.values(captureFeedback);
    if (!entries.length) return;
    const timer = setTimeout(() => {
      setCaptureFeedback(current => Object.fromEntries(Object.entries(current).filter(([, item]) => item.expires > Date.now())));
    }, Math.max(0, Math.min(...entries.map(item => item.expires)) - Date.now()));
    return () => clearTimeout(timer);
  }, [captureFeedback]);
  const groups = new Map<string, Group>();
  for (const account of records) { const group = groups.get(account.normalizedOrigin) ?? { origin: account.normalizedOrigin, name: account.name, url: account.url, accounts: [] }; group.accounts.push(account); groups.set(group.origin, group); }
  const search = query.trim().toLowerCase();
  const visible = [...groups.values()].map(group => {
    const targetMatch = (group.name + ' ' + group.origin).toLowerCase().includes(search);
    return { ...group, accounts: !search || targetMatch ? group.accounts : group.accounts.filter(account => (account.accountName + ' ' + account.username).toLowerCase().includes(search)) };
  }).filter(group => group.accounts.length > 0);
  async function save(draft: AccountDraft) {
    if (flight.current || !editor) return;
    flight.current = true;
    loadVersion.current++;
    try {
      const saved = await tauri.saveWebsite({ id: editor.account?.id, name: editor.group?.name ?? draft.websiteName.trim(), url: editor.group?.url ?? draft.url.trim(), account_name: draft.name.trim(), username: draft.username.trim(), password: draft.password || undefined, notes: editor.account?.notes ?? '' });
      await load(); setExpanded(current => [...current, saved.normalizedOrigin]); setHighlight(saved.id);
      setNotice(saved.name + ' · ' + w('已保存', 'Saved ') + ' ' + saved.accountName);
    } finally { flight.current = false; }
  }
  async function copy(account: Website, password: boolean) {
    setError('');
    try { if (password) await tauri.copyWebsitePassword(account.id); else { const value = await tauri.copyWebsiteUsername(account.id); await navigator.clipboard.writeText(value); } }
    catch (reason) { if (reason instanceof TauriCommandError && reason.code === 'credential.cancelled') setNotice(w('已取消复制密码', 'Password copy cancelled')); else showError(reason); throw reason; }
  }
  return <section className="workspace-page">
    <WorkspaceHeader title={w('网站账号', 'Website accounts')} description={w('你的登录凭据，井然有序。', 'Your sign-ins, thoughtfully organized.')} search={query} setSearch={setQuery} actions={<><PluginConnectionStatus /><IconButton icon="plugin" label={w('浏览器插件', 'Browser extension')} onClick={() => setPlugin(true)} /><button onClick={() => setEditor({})}><Icon name="plus" />{w('添加网站', 'Add website')}</button></>} />
    {error && <p className="workspace-alert" role="alert">{error}</p>}{notice && <p className="workspace-notice" role="status">{notice}</p>}
    {Object.entries(captureFeedback).map(([id, item]) => <p key={id} className="workspace-notice" role="status">{item.text}</p>)}
    <div className="workspace-count">{w('已保存', 'Saved')} · {groups.size} {w('个网站', 'websites')}</div>
    {loading ? <p className="workspace-empty">{w('正在加载…', 'Loading…')}</p> : !visible.length ? <div className="workspace-empty"><h2>{search ? w('没有找到匹配的账号', 'No matching accounts') : w('从第一个网站开始', 'Start with your first website')}</h2><p>{w('手动添加账号，或使用浏览器插件保存。', 'Add an account manually or save one with the browser extension.')}</p></div> : <div className="target-grid">{visible.map(group => {
      const all = Boolean(search) || expanded.includes(group.origin);
      return <article className="target-card" key={group.origin}><header className="target-card__header"><WebsiteIcon name={group.name} url={group.url} /><div className="target-card__identity"><h2>{group.name}</h2><span>{new URL(group.origin).host}</span></div><div className="target-card__actions"><IconButton icon="plus" label={w('添加账号', 'Add account') + ' · ' + group.name} onClick={() => setEditor({ group })} /><IconButton icon="open" label={w('打开', 'Open') + ' ' + group.name} onClick={() => void openUrl(group.url).catch(showError)} /><MoreMenu label={w('网站操作', 'Website actions') + ' · ' + group.name} items={[{ label: w('编辑网站', 'Edit website'), action: () => setEditingGroup(group) }]} /></div></header>
      {(all ? group.accounts : group.accounts.slice(0, 3)).map(account => <AccountRow key={account.id} name={account.accountName} username={account.username} highlight={highlight === account.id || Boolean(captureFeedback[account.id])} onCopy={password => copy(account, password)} onEdit={() => setEditor({ group, account })} onDelete={() => { setError(''); setDeleting(account); }} />)}
      {group.accounts.length > 3 && <button className="target-card__expand" onClick={() => setExpanded(current => all ? current.filter(origin => origin !== group.origin) : [...current, group.origin])}>{all ? w('收起账号', 'Show less') : w('查看其余 ', 'Show remaining ') + (group.accounts.length - 3) + w(' 个账号', ' accounts')}</button>}
      </article>;
    })}</div>}
    {editor && <AccountDrawer title={editor.account ? w('编辑账号', 'Edit account') : editor.group ? w('添加账号', 'Add account') : w('添加网站', 'Add website')} target={editor.group?.name} scope={editor.group ? 'website:' + editor.group.origin : undefined} websiteFields={!editor.group} initial={editor.account ? { name: editor.account.accountName, username: editor.account.username } : undefined} onSave={save} onClose={() => setEditor(undefined)} onDelete={editor.account ? () => setDeleting(editor.account) : undefined} />}
    {editingGroup && <Drawer title={w('编辑网站', 'Edit website')} onClose={() => setEditingGroup(undefined)} busy={busy}><form className="workspace-form" onSubmit={async event => { event.preventDefault(); loadVersion.current++; setBusy(true); try { await tauri.editWebsiteGroup(editingGroup.origin, editingGroup.name, editingGroup.url); await load(); setEditingGroup(undefined); } catch (reason) { showError(reason); } finally { setBusy(false); } }}><label>{w('网站名称', 'Website name')}<input required value={editingGroup.name} onChange={event => setEditingGroup({ ...editingGroup, name: event.target.value })} /></label><label>{w('网站地址', 'Website URL')}<input type="url" required value={editingGroup.url} onChange={event => setEditingGroup({ ...editingGroup, url: event.target.value })} /></label>{error && <p role="alert">{error}</p>}<footer className="workspace-form__footer"><span /><button disabled={busy}>{w('保存', 'Save')}</button></footer></form></Drawer>}
    {deleting && <Drawer title={w('删除账号', 'Delete account')} subtitle={deleting.name} busy={busy} onClose={() => setDeleting(undefined)}><p>{w('将删除', 'Delete')}“{deleting.accountName}” ({deleting.username}) {w('及其保存的密码。此操作无法撤销。', 'and its saved password. This cannot be undone.')}</p>{groups.get(deleting.normalizedOrigin)?.accounts.length === 1 && <p>{w('这是最后一个账号，删除后网站卡片也会消失。', 'This is the last account. The website card will also be removed.')}</p>}{error && <p role="alert">{error}</p>}<div className="workspace-form__footer"><button className="secondary-button" disabled={busy} onClick={() => setDeleting(undefined)}>{w('取消', 'Cancel')}</button><button className="danger" disabled={busy} onClick={async () => { if (flight.current) return; flight.current = true; loadVersion.current++; setBusy(true); try { await tauri.deleteWebsite(deleting.id); await load(); setNotice(deleting.name + ' · ' + w('已删除', 'Deleted ') + deleting.accountName); setDeleting(undefined); setEditor(undefined); } catch (reason) { showError(reason); } finally { setBusy(false); flight.current = false; } }}>{w('删除账号', 'Delete account')}</button></div></Drawer>}
    {plugin && <PluginGuide onClose={() => setPlugin(false)} />}
  </section>;
}
