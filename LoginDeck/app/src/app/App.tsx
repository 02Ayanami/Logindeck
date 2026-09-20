import { useEffect, useState } from 'react';
import { initializeLocale } from './i18n';
import { AppShell, type Page } from '../components/shell/AppShell';
import { WebsiteList } from '../features/websites/WebsiteList';
import { ApplicationLibrary } from '../features/applications/ApplicationLibrary';
import { SettingsPage } from '../features/settings/SettingsPage';
import { ButtonTooltips } from '../components/ui/ButtonTooltips';

export function App() {
  const [page, setPage] = useState<Page>('websites');

  useEffect(() => {
    void initializeLocale();
  }, []);

  return (
    <AppShell page={page} onNavigate={setPage}>
      <ButtonTooltips />
      {page === 'websites' && <WebsiteList />}
      {page === 'applications' && <ApplicationLibrary />}
      {page === 'settings' && <SettingsPage />}
    </AppShell>
  );
}
