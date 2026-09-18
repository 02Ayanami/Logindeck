import { createElement } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

import en from '../locales/en.json';
import zhCN from '../locales/zh-CN.json';
import { flattenKeys, i18n, initializeLocale, recordLocaleChoice } from './i18n';
import { LanguageSetting } from '../features/settings/LanguageSetting';

describe('localization', () => {
  beforeEach(async () => {
    invokeMock.mockReset();
    await i18n.changeLanguage('en');
  });

  it('keeps English and Chinese translation keys identical', () => {
    expect(flattenKeys(zhCN)).toEqual(flattenKeys(en));
  });

  it('starts safely in English before settings load and applies a valid saved locale', async () => {
    invokeMock.mockResolvedValue({ ui_locale: 'zh-CN' });
    expect(i18n.language).toBe('en');
    await initializeLocale();
    expect(i18n.language).toBe('zh-CN');
    expect(invokeMock).toHaveBeenCalledWith('get_settings');
  });

  it('does not let a late settings response overwrite a user locale choice', async () => {
    let resolveSettings!: (value: { ui_locale: 'en' }) => void;
    invokeMock.mockImplementation(() => new Promise(resolve => { resolveSettings = resolve; }));
    const pending = initializeLocale();
    await i18n.changeLanguage('zh-CN');
    recordLocaleChoice();
    resolveSettings({ ui_locale: 'en' });
    await pending;
    expect(i18n.language).toBe('zh-CN');
  });

  it('does not let a late settings failure overwrite a user locale choice', async () => {
    let rejectSettings!: (reason: unknown) => void;
    invokeMock.mockImplementation(() => new Promise((_, reject) => { rejectSettings = reject; }));
    const pending = initializeLocale();
    await i18n.changeLanguage('zh-CN');
    recordLocaleChoice();
    rejectSettings(new Error('offline'));
    await pending;
    expect(i18n.language).toBe('zh-CN');
  });

  it('switches immediately and persists the selected locale', async () => {
    invokeMock.mockResolvedValue({ ui_locale: 'zh-CN' });
    render(createElement(LanguageSetting));

    expect(screen.getByRole('heading', { name: 'Language' })).toBeVisible();
    await userEvent.selectOptions(screen.getByRole('combobox', { name: 'Language' }), 'zh-CN');

    expect(screen.getByRole('heading', { name: '语言' })).toBeVisible();
    expect(invokeMock).toHaveBeenCalledWith('set_locale', { locale: 'zh-CN' });
  });

  it('rolls back a failed locale save and reports a safe error', async () => {
    invokeMock.mockRejectedValue({ code: 'storage.database', params: {} });
    render(createElement(LanguageSetting));

    await userEvent.selectOptions(screen.getByRole('combobox', { name: 'Language' }), 'zh-CN');

    expect(screen.getByRole('heading', { name: 'Language' })).toBeVisible();
    expect(screen.getByRole('alert')).toHaveTextContent('Unable to save language preference.');
  });
});
