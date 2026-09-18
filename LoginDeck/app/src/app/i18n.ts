import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';
import { tauri } from '../lib/tauri';

export const supportedLocales = ['en', 'zh-CN'] as const;

let localeRevision = 0;
export function recordLocaleChoice(): void { localeRevision += 1; }

void i18n.use(initReactI18next).init({
  resources: { en: { translation: en }, 'zh-CN': { translation: zhCN } },
  lng: 'en',
  fallbackLng: 'en',
  supportedLngs: supportedLocales,
  interpolation: { escapeValue: false },
});

export async function initializeLocale(): Promise<void> {
  const revisionAtStart = localeRevision;
  try {
    const settings = await tauri.getSettings();
    if (revisionAtStart === localeRevision) await i18n.changeLanguage(settings.uiLocale);
  } catch {
    // The safe English default remains usable when settings are unavailable or malformed.
    if (revisionAtStart === localeRevision) await i18n.changeLanguage('en');
  }
}

export function flattenKeys(value: object, prefix = ''): string[] {
  return Object.entries(value).flatMap(([key, child]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return child !== null && typeof child === 'object' ? flattenKeys(child as object, path) : [path];
  }).sort();
}

export function translateCommandError(code: string, params: Record<string, string>): string {
  const key = `errors.${code}`;
  return i18n.exists(key) ? i18n.t(key, params) : i18n.t('errors.internal.error');
}

export { i18n };
