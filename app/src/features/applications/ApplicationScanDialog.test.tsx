import { fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';
import '../../app/i18n';
import { ApplicationScanDialog } from './ApplicationScanDialog';
import type { ScanStatus } from '../../lib/tauri';
const snapshot: ScanStatus = { id: 1, phase: 'choosing', started_at: 1, count: 3, error: null, candidates: [
  { token: 0, display_name: 'QQ', selected: true },
  { token: 1, display_name: 'WeChat', selected: false },
  { token: 2, display_name: 'Steam', selected: false },
] };
it('starts from saved membership, toggles add and delete, and submits the final selection', async () => {
 const confirm = vi.fn(), cancel = vi.fn();
 render(<ApplicationScanDialog snapshot={snapshot} busy={false} onConfirm={confirm} onCancel={cancel} />);
 expect(screen.getByRole('checkbox', { name: 'Select QQ' })).toBeChecked();
 expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled();
 await userEvent.click(screen.getByText('QQ'));
 expect(screen.getByRole('checkbox', { name: 'Select QQ' })).not.toBeChecked();
 await userEvent.click(screen.getByText('QQ'));
 expect(screen.getByRole('checkbox', { name: 'Select QQ' })).toBeChecked();
 await userEvent.click(screen.getByText('QQ'));
 await userEvent.click(screen.getByText('WeChat'));
 expect(confirm).not.toHaveBeenCalled();
 await userEvent.click(screen.getByRole('button', { name: 'Save changes' }));
 expect(confirm).toHaveBeenCalledWith([1]);
});
it('changes only visible apps and preserves selections across search changes', async () => {
 render(<ApplicationScanDialog snapshot={snapshot} busy={false} onConfirm={vi.fn()} onCancel={vi.fn()} />);
 const search = screen.getByRole('searchbox');
 await userEvent.type(search, 'QQ');
 await userEvent.click(screen.getByRole('button', { name: 'Deselect visible' }));
 await userEvent.clear(search);
 expect(screen.getByRole('checkbox', { name: 'Select QQ' })).not.toBeChecked();
 expect(screen.getByRole('checkbox', { name: 'Select WeChat' })).not.toBeChecked();
 await userEvent.click(screen.getByRole('button', { name: 'Select visible' }));
 expect(screen.getByRole('status')).toHaveTextContent('3 apps selected');
 expect(screen.getByRole('checkbox', { name: 'Select Steam' })).toBeChecked();
 await userEvent.type(search, 'WeChat');
 await userEvent.click(screen.getByRole('button', { name: 'Deselect visible' }));
 expect(screen.getByRole('status')).toHaveTextContent('2 apps selected');
});
it('cancel and Escape discard selection without confirming, but committing blocks dismissal', async () => {
 const confirm = vi.fn(), cancel = vi.fn(), parent = vi.fn();
 const view = render(<div onKeyDown={parent}><ApplicationScanDialog snapshot={snapshot} busy={false} onConfirm={confirm} onCancel={cancel} /></div>);
 await userEvent.click(screen.getByText('QQ'));
 await userEvent.keyboard('{Escape}');
 expect(cancel).toHaveBeenCalledOnce(); expect(parent).not.toHaveBeenCalled(); expect(confirm).not.toHaveBeenCalled();
 view.rerender(<ApplicationScanDialog snapshot={{...snapshot, phase:'committing', candidates:[]}} busy onConfirm={confirm} onCancel={cancel} />);
 fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
 expect(cancel).toHaveBeenCalledOnce();
 expect(within(screen.getByRole('dialog')).getByRole('button', { name: 'Cancel' })).toBeDisabled();
});
it('handles an empty scan without an enabled add action', () => {
 render(<ApplicationScanDialog snapshot={{...snapshot,candidates:[],count:0}} busy={false} onConfirm={vi.fn()} onCancel={vi.fn()} />);
 expect(screen.getByText('No apps were discovered. You can also import an app manually.')).toBeVisible();
 expect(screen.getByRole('button', {name:'Save changes'})).toBeDisabled();
});
