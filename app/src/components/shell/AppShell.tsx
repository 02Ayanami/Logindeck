import type { MouseEvent, ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { BrandMark } from '../brand/BrandMark';

export type Page = 'websites' | 'applications' | 'settings';

export type AppShellProps = {
  page: Page;
  onNavigate: (page: Page) => void;
  children: ReactNode;
};

const destinations: Page[] = ['websites', 'applications', 'settings'];

function NavigationIcon({ page }: { page: Page }) {
  if (page === 'websites') {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="12" cy="12" r="8.5" />
        <path d="M3.8 9h16.4M3.8 15h16.4M12 3.5c2.1 2.3 3.2 5.1 3.2 8.5S14.1 18.2 12 20.5C9.9 18.2 8.8 15.4 8.8 12S9.9 5.8 12 3.5Z" />
      </svg>
    );
  }

  if (page === 'applications') {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <rect x="3.5" y="3.5" width="6.5" height="6.5" rx="1.5" />
        <rect x="14" y="3.5" width="6.5" height="6.5" rx="1.5" />
        <rect x="3.5" y="14" width="6.5" height="6.5" rx="1.5" />
        <rect x="14" y="14" width="6.5" height="6.5" rx="1.5" />
      </svg>
    );
  }

  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.2 13.2a7.6 7.6 0 0 0 0-2.4l2-1.5-2-3.4-2.4 1a8.7 8.7 0 0 0-2-1.2L14.5 3h-4l-.3 2.7a8.7 8.7 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.5a7.6 7.6 0 0 0 0 2.4l-2 1.5 2 3.4 2.4-1a8.7 8.7 0 0 0 2 1.2l.3 2.7h4l.3-2.7a8.7 8.7 0 0 0 2-1.2l2.4 1 2-3.4-2-1.5Z" />
    </svg>
  );
}

export function AppShell({ page, onNavigate, children }: AppShellProps) {
  const { t } = useTranslation();

  const navigate = (event: MouseEvent<HTMLAnchorElement>, destination: Page) => {
    event.preventDefault();
    onNavigate(destination);
  };

  return (
    <div className="app-shell" data-layout="desktop">
      <a className="skip-link" href="#main-content">
        {t('app.skipToContent')}
      </a>
      <aside className="sidebar">
        <div className="sidebar__brand">
          <BrandMark className="sidebar__brand-mark" />
          <div>
            <strong>{t('app.title')}</strong>
          </div>
        </div>

        <nav className="sidebar__nav" aria-label={t('app.navigation')}>
          {destinations.map((destination) => (
            <a
              href={`#${destination}`}
              key={destination}
              className={destination === 'settings' ? 'sidebar__settings' : undefined}
              aria-current={page === destination ? 'page' : undefined}
              onClick={(event) => navigate(event, destination)}
            >
              <NavigationIcon page={destination} />
              <span>{t(`app.${destination}`)}</span>
            </a>
          ))}
        </nav>
        <div className="sidebar__footer"><span>{t('app.securityFooter')}</span></div>
      </aside>

      <main className="main-region" id="main-content" tabIndex={-1}>
        {children}
      </main>
    </div>
  );
}
