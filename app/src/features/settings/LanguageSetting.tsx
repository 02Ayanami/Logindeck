import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { i18n, recordLocaleChoice, translateCommandError } from '../../app/i18n';
import { tauri, type Locale } from '../../lib/tauri';

export function LanguageSetting() {
  const { t } = useTranslation();
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const locale = i18n.resolvedLanguage === 'zh-CN' ? 'zh-CN' : 'en';

  async function changeLocale(next: Locale) {
    if (next === locale || saving) return;
    const previous = locale;
    setError(undefined);
    recordLocaleChoice();
    await i18n.changeLanguage(next);
    setSaving(true);
    try {
      await tauri.setLocale(next);
    } catch (reason) {
      await i18n.changeLanguage(previous);
      const message = reason instanceof Error && 'code' in reason
        ? translateCommandError(String(reason.code), (reason as { params?: Record<string, string> }).params ?? {})
        : t('settings.languageSaveFailed');
      setError(message);
    } finally {
      setSaving(false);
    }
  }

  return <section className="language-setting" aria-labelledby="language-heading">
    <div className="settings-card__icon" aria-hidden="true">
      <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.5" /><path d="M3.8 9h16.4M3.8 15h16.4M12 3.5c2.1 2.3 3.2 5.1 3.2 8.5S14.1 18.2 12 20.5C9.9 18.2 8.8 15.4 8.8 12S9.9 5.8 12 3.5Z" /></svg>
    </div>
    <div className="settings-card__heading"><h2 id="language-heading">{t('settings.language')}</h2><p>{t('settings.languageDescription')}</p></div>
    <label>
      <span>{t('settings.language')}</span>
      <select aria-label={t('settings.language')} value={locale} disabled={saving} onChange={(event) => void changeLocale(event.target.value as Locale)}>
        <option value="en">{t('settings.english')}</option>
        <option value="zh-CN">{t('settings.chinese')}</option>
      </select>
    </label>
    {error && <p role="alert" aria-live="assertive">{error}</p>}
  </section>;
}
