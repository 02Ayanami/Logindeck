import { useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import type { ScanStatus } from '../../lib/tauri';
import { ScanCandidateIcon } from './ScanCandidateIcon';

export function ApplicationScanDialog({ snapshot, busy, error, onConfirm, onCancel }: {
  snapshot: ScanStatus; busy: boolean; error?: string;
  onConfirm: (tokens: number[]) => void; onCancel: () => void;
}) {
  const { t } = useTranslation();
  // Keep the discovered list visible while the backend commits and clears its candidates.
  const [candidates] = useState(snapshot.candidates);
  const [selected, setSelected] = useState<Set<number>>(() => new Set(candidates.filter((app) => app.selected).map((app) => app.token)));
  const [search, setSearch] = useState('');
  const panel = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    searchRef.current?.focus();
    return () => { queueMicrotask(() => { if (previous?.isConnected) previous.focus(); }); };
  }, []);
  const query = search.trim().toLowerCase();
  const visible = candidates.filter((app) => app.display_name.toLowerCase().includes(query));
  const available = visible;
  const allSelected = available.length > 0 && available.every((app) => selected.has(app.token));
  const changed = candidates.some((app) => selected.has(app.token) !== app.selected);
  function toggle(token: number) {
    if (busy) return;
    setSelected((current) => { const next = new Set(current); if (next.has(token)) next.delete(token); else next.add(token); return next; });
  }
  return createPortal(<div className="confirm-dialog__backdrop"><div ref={panel} tabIndex={-1} className="application-scan-dialog" role="dialog" aria-modal="true" aria-labelledby="scan-choice-title" aria-describedby="scan-choice-description" aria-busy={busy} onKeyDown={(event) => {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); if (!busy) onCancel(); }
    if (event.key === 'Tab') {
      const controls = Array.from(panel.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled)') ?? []);
      const first = controls[0], last = controls.at(-1);
      if (!first) { event.preventDefault(); panel.current?.focus(); }
      else if (event.shiftKey && (document.activeElement === first || document.activeElement === panel.current)) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && (document.activeElement === last || document.activeElement === panel.current)) { event.preventDefault(); first.focus(); }
    }
  }}>
    <header className="application-scan-dialog__header">
      <div><span className="application-scan-dialog__eyebrow">{t('applications.scanChoice.eyebrow')}</span><h2 id="scan-choice-title">{t('applications.scanChoice.title')}</h2><p id="scan-choice-description">{t('applications.scanChoice.description')}</p></div>
      <button type="button" className="application-scan-dialog__close" aria-label={t('applications.scanChoice.close')} disabled={busy} onClick={onCancel}>×</button>
    </header>
    <div className="application-scan-dialog__tools">
      <label className="search-control"><span className="visually-hidden">{t('applications.scanChoice.search')}</span><input ref={searchRef} type="search" value={search} disabled={busy} placeholder={t('applications.scanChoice.search')} aria-label={t('applications.scanChoice.search')} onChange={(event) => setSearch(event.target.value)} /></label>
      <button type="button" className="row-quiet-action" disabled={busy || available.length === 0} onClick={() => setSelected((current) => {
        const next = new Set(current); for (const app of available) { if (allSelected) next.delete(app.token); else next.add(app.token); } return next;
      })}>{t(allSelected ? 'applications.scanChoice.clearVisible' : 'applications.scanChoice.selectVisible')}</button>
    </div>
    <div className="application-scan-dialog__body">
      {candidates.length === 0 ? <p className="application-scan-dialog__empty">{t('applications.scanChoice.empty')}</p>
        : visible.length === 0 ? <p className="application-scan-dialog__empty">{t('applications.scanChoice.noResults')}</p>
        : <ul className="application-scan-grid" aria-label={t('applications.scanChoice.results')}>{visible.map((app) => <li key={app.token}>
          <label className="application-scan-card" data-selected={selected.has(app.token)}>
            <input type="checkbox" checked={selected.has(app.token)} disabled={busy} aria-label={t('applications.scanChoice.selectApp', { name: app.display_name })} onChange={() => toggle(app.token)} />
            <ScanCandidateIcon scanId={snapshot.id} token={app.token} name={app.display_name} />
            <strong>{app.display_name}</strong>
            <span className="application-scan-card__check" aria-hidden="true">{selected.has(app.token) ? '✓' : ''}</span>
          </label>
        </li>)}</ul>}
    </div>
    <footer className="application-scan-dialog__footer">
      <div><p role="status">{snapshot.phase === 'committing' ? t('applications.scanPhase.committing') : t('applications.scanChoice.selected', { count: selected.size })}</p>{error && <p className="notice error" role="alert">{error}</p>}</div>
      <div className="application-scan-dialog__actions"><button type="button" disabled={busy} onClick={onCancel}>{t('applications.cancel')}</button><button type="button" disabled={busy || !changed} onClick={() => onConfirm([...selected])}>{t('applications.scanChoice.confirm')}</button></div>
    </footer>
  </div></div>, document.body);
}
