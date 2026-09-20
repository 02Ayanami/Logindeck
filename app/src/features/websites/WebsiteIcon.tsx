import { useState } from 'react';

// Request only the site's conventional public icon. Never send the saved login URL,
// its query parameters, username, or password to an external favicon service.
export function faviconUrl(value: string): string | undefined {
  try {
    const url = new URL(value);
    if (url.protocol !== 'https:' || url.username || url.password) return undefined;
    return `${url.origin}/favicon.ico`;
  } catch { return undefined; }
}

export function WebsiteIcon({ name, url, detail = false }: { name: string; url: string; detail?: boolean }) {
  const src = faviconUrl(url);
  // Remount when the origin changes so a previous load/error cannot affect another site.
  return <Icon key={src ?? url} name={name} src={src} detail={detail} />;
}

function Icon({ name, src, detail }: { name: string; src?: string; detail: boolean }) {
  const [state, setState] = useState<'loading' | 'loaded' | 'failed'>('loading');
  return <span className={`${detail ? 'detail-hero__mark' : 'website-row__monogram'} website-icon`} aria-hidden="true">
    {state !== 'loaded' && <span>{name.trim().charAt(0).toUpperCase() || '?'}</span>}
    {src && state !== 'failed' && <img src={src} alt="" width={32} height={32} loading="lazy" decoding="async" referrerPolicy="no-referrer" draggable={false}
      className={state === 'loaded' ? 'website-icon__image' : 'website-icon__image website-icon__image--loading'}
      onLoad={() => setState('loaded')} onError={() => setState('failed')} />}
  </span>;
}
