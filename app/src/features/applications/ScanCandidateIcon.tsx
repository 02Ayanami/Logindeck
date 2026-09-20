import { useEffect, useState } from 'react';
import { tauri } from '../../lib/tauri';

const icons = new Map<string, Promise<string | null>>();

export function ScanCandidateIcon({ scanId, token, name }: {
  scanId: number; token: number; name: string;
}) {
  const key = `${scanId}:${token}`;
  const [url, setUrl] = useState<string | null>();
  useEffect(() => {
    let live = true;
    let request = icons.get(key);
    if (!request) {
      if (icons.size >= 4096) icons.clear();
      request = tauri.getScanCandidateIcon(scanId, token).catch(() => null);
      icons.set(key, request);
    }
    void request.then((value) => { if (live) setUrl(value); });
    return () => { live = false; };
  }, [key, scanId, token]);
  return <span className={`application-scan-card__icon${url ? ' application-scan-card__icon--image' : ''}`} aria-hidden="true">
    {url ? <img src={url} alt="" width={72} height={72} draggable={false} onError={() => setUrl(null)} /> : name.trim().charAt(0).toUpperCase() || '?'}
  </span>;
}
