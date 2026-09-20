import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { translateCommandError } from '../../app/i18n';
import { PageHeader } from '../../components/ui/PageHeader';
import { tauri, TauriCommandError, type BrowserCaptureSettings } from '../../lib/tauri';
import { LanguageSetting } from './LanguageSetting';
import { PluginConnectionStatus } from './PluginConnectionStatus';
import { PluginGuide } from './PluginGuide';

export function SettingsPage() {
  const { t } = useTranslation();
  const [capture, setCapture] = useState<BrowserCaptureSettings>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [guide, setGuide] = useState(false);

  useEffect(() => {
    let active = true;
    void tauri.getBrowserCaptureSettings()
      .then((value) => { if (active) setCapture(value); })
      .catch((reason) => {
        if (!active) return;
        setError(reason instanceof TauriCommandError
          ? translateCommandError(reason.code, reason.params)
          : t('settings.browserCaptureLoadFailed'));
      });
    return () => { active = false; };
  }, [t]);

  async function toggleCapture() {
    if (!capture || busy) return;
    setBusy(true);
    setError('');
    try {
      setCapture(await tauri.setBrowserCaptureEnabled(!capture.enabled));
    } catch (reason) {
      setError(reason instanceof TauriCommandError
        ? translateCommandError(reason.code, reason.params)
        : t('settings.browserCaptureSaveFailed'));
    } finally {
      setBusy(false);
    }
  }

  return <section className="settings-page workspace-page">
    <PageHeader title={t('settings.title')} description={t('settings.description')} />
    <div className="settings-grid settings-grid--simple">
      <LanguageSetting />
      <section className="settings-card browser-capture-settings" aria-labelledby="browser-capture-heading">
        <div className="browser-capture-row">
          <div>
            <h2 id="browser-capture-heading">{t('settings.browserCapture')}</h2>
            <p>{t('settings.browserCaptureDescription')}</p>
          </div>
          <button
            type="button"
            className="settings-switch"
            role="switch"
            aria-label={t('settings.browserCapture')}
            aria-checked={capture?.enabled ?? false}
            disabled={!capture || busy}
            onClick={() => void toggleCapture()}
          >
            <span aria-hidden="true" />
          </button>
        </div>
        <p className="browser-capture-connection"><PluginConnectionStatus /></p>
        <button type="button" className="secondary-button" onClick={() => setGuide(true)}>
          {t('settings.browserCaptureHelp')}
        </button>
        {error && <p role="alert">{error}</p>}
      </section>
    </div>
    {guide && <PluginGuide onClose={() => setGuide(false)} />}
  </section>;
}
