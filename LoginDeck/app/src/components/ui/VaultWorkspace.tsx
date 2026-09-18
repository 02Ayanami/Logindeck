import { useEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';

export function useWords() {
  const { i18n } = useTranslation();
  return (zh: string, en: string) => i18n.resolvedLanguage === 'zh-CN' ? zh : en;
}
export type IconName = 'plus' | 'open' | 'copy' | 'key' | 'more' | 'close' | 'search' | 'plugin' | 'check' | 'arrow';
export function Icon({ name }: { name: IconName }) {
  const paths: Record<IconName, ReactNode> = {
    plus: <path d="M12 5v14M5 12h14" />,
    open: <><path d="M13 5h6v6M19 5l-9 9" /><path d="M10 5H6a1 1 0 0 0-1 1v12a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-4" /></>,
    copy: <><rect x="8" y="8" width="11" height="12" rx="2" /><path d="M15 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h3" /></>,
    key: <><circle cx="8" cy="11" r="4" /><path d="M12 11h9m-3 0v4m-3-4v3" /></>,
    more: <><circle cx="5" cy="12" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="19" cy="12" r="1" /></>,
    close: <path d="m6 6 12 12M18 6 6 18" />,
    search: <><circle cx="10" cy="10" r="6.5" /><path d="m15 15 5 5" /></>,
    plugin: <><path d="M8 3v5m8-5v5M6 8h12v3a6 6 0 0 1-6 6v4M6 8v3a6 6 0 0 0 6 6" /></>,
    check: <path d="m5 12 4 4L19 6" />,
    arrow: <path d="m11 5-7 7 7 7M4 12h16" />,
  };
  return <svg viewBox="0 0 24 24" aria-hidden="true">{paths[name]}</svg>;
}
export function IconButton({ icon, label, onClick, disabled }: { icon: IconName; label: string; onClick: () => void; disabled?: boolean }) {
  return <button type="button" className="icon-button" aria-label={label} disabled={disabled} onClick={onClick}><Icon name={icon} /></button>;
}
export function MoreMenu({ label, items }: { label: string; items: { label: string; action: () => void; danger?: boolean }[] }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => { if (!root.current?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener('pointerdown', close);
    return () => document.removeEventListener('pointerdown', close);
  }, [open]);
  return <div className="workspace-menu" ref={root} onKeyDown={event => { if (event.key === 'Escape') setOpen(false); }}>
    <button className="icon-button" type="button" aria-label={label} aria-expanded={open} onClick={() => setOpen(!open)}><Icon name="more" /></button>
    {open && <div className="workspace-menu__items">{items.map(item => <button key={item.label} className={item.danger ? 'text-danger' : ''} onClick={() => { setOpen(false); item.action(); }}>{item.label}</button>)}</div>}
  </div>;
}
export function Drawer({ title, subtitle, children, onClose, busy = false }: { title: string; subtitle?: string; children: ReactNode; onClose: () => void; busy?: boolean }) {
  const panel = useRef<HTMLElement>(null);
  const w = useWords();
  useEffect(() => {
    const previous = document.activeElement as HTMLElement;
    const overflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    panel.current?.focus();
    return () => { document.body.style.overflow = overflow; previous?.focus(); };
  }, []);
  return createPortal(<div className="workspace-overlay" onClick={event => { if (event.target === event.currentTarget && !busy) onClose(); }}>
    <section className="workspace-drawer" ref={panel} tabIndex={-1} role="dialog" aria-modal="true" aria-label={title} onKeyDown={event => {
      if (event.key === 'Escape' && !busy) { event.stopPropagation(); onClose(); }
      if (event.key === 'Tab') {
        const controls = Array.from(panel.current?.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),a[href],textarea:not(:disabled)') ?? []);
        const first = controls[0], last = controls.at(-1);
        if (event.shiftKey && (document.activeElement === first || document.activeElement === panel.current)) { event.preventDefault(); last?.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }
    }}>
      <header><div><h2>{title}</h2>{subtitle && <p>{subtitle}</p>}</div><IconButton icon="close" label={w('关闭', 'Close')} disabled={busy} onClick={onClose} /></header>
      <div className="workspace-drawer__body">{children}</div>
    </section>
  </div>, document.body);
}
export function WorkspaceHeader({ title, description, search, setSearch, actions }: { title: string; description: string; search: string; setSearch: (s: string) => void; actions: ReactNode }) {
  const w = useWords();
  return <header className="workspace-header"><div className="workspace-heading"><div><h1>{title}</h1><p>{description}</p></div><div className="workspace-heading__actions">{actions}</div></div>
    <label className="workspace-search"><Icon name="search" /><input type="search" aria-label={w('搜索', 'Search')} placeholder={w('搜索名称或登录账号', 'Search names or login accounts')} value={search} onChange={event => setSearch(event.target.value)} /></label>
  </header>;
}
export function AccountRow({ name, username, onCopy, onEdit, onDelete, passwordAvailable = true, highlight = false }: { name: string; username: string; onCopy: (password: boolean) => Promise<void>; onEdit: () => void; onDelete: () => void; passwordAvailable?: boolean; highlight?: boolean }) {
  const w = useWords();
  const [copied, setCopied] = useState<'copy' | 'key'>();
  const [busy, setBusy] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  async function copy(password: boolean) {
    if (busy) return;
    setBusy(true);
    try { await onCopy(password); setCopied(password ? 'key' : 'copy'); clearTimeout(timer.current); timer.current = setTimeout(() => setCopied(undefined), 2000); }
    catch { /* The parent displays a localized failure. */ }
    finally { setBusy(false); }
  }
  return <div className="credential-row" data-highlight={highlight}>
    <div className="credential-row__content"><strong>{name}</strong><div className="credential-row__login"><span title={username}>{username || w('尚未填写登录账号', 'Login account missing')}</span>
      <IconButton icon={copied === 'copy' ? 'check' : 'copy'} label={w('复制账号', 'Copy account') + ' · ' + name} disabled={busy || !username} onClick={() => void copy(false)} />
      <IconButton icon={copied === 'key' ? 'check' : 'key'} label={w('复制密码', 'Copy password') + ' · ' + name} disabled={busy || !passwordAvailable} onClick={() => void copy(true)} />
    </div></div>
    <MoreMenu label={w('账号操作', 'Account actions') + ' · ' + name} items={[{ label: w('编辑账号', 'Edit account'), action: onEdit }, { label: w('删除账号', 'Delete account'), action: onDelete, danger: true }]} />
  </div>;
}
