import { useEffect, useState } from 'react';
import { tauri, type Application } from '../../lib/tauri';

// Shared between list and detail views. Each mounted library owns a revision that invalidates
// stale results after scan/import; failed reads remain a normal monogram fallback.
const icons = new Map<string, Promise<string | null>>();
let active = 0;
const pending: (() => void)[] = [];
function requestIcon(id: string): Promise<string | null> {
  return new Promise((resolve) => {
    const start = () => {
      active += 1;
      void tauri.getApplicationIcon(id).catch(() => null).then(resolve).finally(() => {
        active -= 1;
        pending.shift()?.();
      });
    };
    if (active < 4) start(); else pending.push(start);
  });
}

export function ApplicationIcon({ application, revision }: { application: Application; revision: number }) {
  const key = JSON.stringify([application.id, application.launchTarget, application.version, application.updatedAt, revision]);
  const [loaded, setLoaded] = useState<{ key: string; url: string | null }>();
  useEffect(() => {
    if (!application.isPresent) return;
    let live = true;
    let request = icons.get(key);
    if (!request) {
      if (icons.size >= 256) icons.delete(icons.keys().next().value!);
      request = requestIcon(application.id);
      icons.set(key, request);
    }
    void request.then((url) => { if (live) setLoaded({ key, url }); });
    return () => { live = false; };
  }, [application.id, application.isPresent, key]);
  const url = application.isPresent && loaded?.key === key ? loaded.url : null;
  return <span className={`application-glyph${url ? ' application-glyph--image' : ''}`} aria-hidden="true">
    {url ? <img src={url} alt="" width={46} height={46} draggable={false} onError={() => setLoaded({ key, url: null })} /> : application.displayName.trim().charAt(0).toUpperCase() || '?'}
  </span>;
}
