import { act, fireEvent, render, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
const { getIcon } = vi.hoisted(() => ({ getIcon: vi.fn() }));
vi.mock('../../lib/tauri', () => ({ tauri: { getApplicationIcon: getIcon } }));
import { ApplicationIcon } from './ApplicationIcon';
import type { Application } from '../../lib/tauri';
const app = { id: 'icon-app', displayName: 'Example', launchTarget: '/Applications/Example.app', version: '1', updatedAt: '1', isPresent: true } as Application;
const png = 'data:image/png;base64,iVBORw0KGgo=';

describe('ApplicationIcon', () => {
 it('shares one request between list and detail, and refreshes on revision changes', async () => {
  getIcon.mockReset(); getIcon.mockResolvedValue(png);
  const { container, rerender } = render(<><ApplicationIcon application={app} revision={10} /><ApplicationIcon application={app} revision={10} /></>);
  await waitFor(() => expect(container.querySelectorAll('img')).toHaveLength(2));
  expect(getIcon).toHaveBeenCalledTimes(1);
  rerender(<ApplicationIcon application={app} revision={11} />);
  await waitFor(() => expect(getIcon).toHaveBeenCalledTimes(2));
  expect(container.querySelector('img')).toHaveAttribute('src', png);
 });
 it('rejects a late result for a previously selected path', async () => {
  getIcon.mockReset(); let finish!: (url: string) => void;
  getIcon.mockImplementationOnce(() => new Promise<string>((resolve) => { finish = resolve; })).mockResolvedValueOnce(null);
  const { container, rerender } = render(<ApplicationIcon application={app} revision={20} />);
  rerender(<ApplicationIcon application={{ ...app, launchTarget: '/Other.app' }} revision={20} />);
  await act(async () => { finish(png); });
  expect(container.querySelector('img')).toBeNull();
  expect(container).toHaveTextContent('E');
 });
 it('falls back for missing applications and corrupt images', async () => {
  getIcon.mockReset(); getIcon.mockResolvedValue(png);
  const { container, rerender } = render(<ApplicationIcon application={app} revision={30} />);
  await waitFor(() => expect(container.querySelector('img')).not.toBeNull());
  fireEvent.error(container.querySelector('img')!);
  expect(container).toHaveTextContent('E');
  rerender(<ApplicationIcon application={{ ...app, isPresent: false }} revision={31} />);
  expect(container.querySelector('img')).toBeNull();
  expect(getIcon).toHaveBeenCalledTimes(1);
 });
});
