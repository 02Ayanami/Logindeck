import { createRef } from 'react';
import { readFileSync } from 'node:fs';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { ConfirmDialog } from './ConfirmDialog';
import { DetailPanel } from './DetailPanel';
import { EmptyState, Notice, SkeletonList } from './Feedback';
import { PageHeader } from './PageHeader';
import { VaultList, VaultRow } from './VaultList';

describe('shared presentation primitives', () => {
  it('renders one page heading and named action regions', () => {
    render(
      <PageHeader
        title="Websites"
        description="Saved sign-ins"
        search={<input aria-label="Search websites" />}
        actions={<button>Add website</button>}
      />,
    );
    expect(screen.getByRole('heading', { level: 1, name: 'Websites' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Add website' })).toBeVisible();
  });

  it('keeps list and row semantics explicit', () => {
    render(
      <VaultList label="Saved websites">
        <VaultRow selected={false}>
          <span>Example</span>
        </VaultRow>
      </VaultList>,
    );
    expect(screen.getByRole('list', { name: 'Saved websites' })).toBeVisible();
    expect(screen.getByRole('listitem')).toHaveTextContent('Example');
  });

  it('uses correct live-region roles and stable skeleton count', () => {
    const { rerender } = render(<Notice kind="success">Saved</Notice>);
    expect(screen.getByRole('status')).toHaveTextContent('Saved');
    rerender(<Notice kind="error">Failed</Notice>);
    expect(screen.getByRole('alert')).toHaveTextContent('Failed');
    rerender(<SkeletonList label="Loading websites" rows={3} />);
    expect(screen.getAllByTestId('skeleton-row')).toHaveLength(3);
  });

  it('provides an actionable empty state', () => {
    render(
      <EmptyState
        title="No websites"
        description="Add your first website."
        action={<button>Add website</button>}
      />,
    );
    expect(screen.getByRole('button', { name: 'Add website' })).toBeVisible();
  });

  it('moves focus into a detail panel and returns it to the opener', () => {
    const opener = createRef<HTMLButtonElement>();
    const { rerender } = render(
      <>
        <button ref={opener}>Open</button>
        <DetailPanel open={false} title="Example" closeLabel="Close details" onClose={() => {}} returnFocusRef={opener}>
          Details
        </DetailPanel>
      </>,
    );
    opener.current?.focus();
    rerender(
      <>
        <button ref={opener}>Open</button>
        <DetailPanel open title="Example" closeLabel="Close details" onClose={() => {}} returnFocusRef={opener}>
          Details
        </DetailPanel>
      </>,
    );
    expect(screen.getByRole('heading', { name: 'Example' })).toHaveFocus();
    expect(screen.getByRole('heading', { name: 'Example' }).closest('section')).toHaveAttribute(
      'data-detail-panel',
    );
    rerender(
      <>
        <button ref={opener}>Open</button>
        <DetailPanel open={false} title="Example" closeLabel="Close details" onClose={() => {}} returnFocusRef={opener}>
          Details
        </DetailPanel>
      </>,
    );
    expect(opener.current).toHaveFocus();
  });

  it('contains dialog focus and ignores Escape while busy', async () => {
    const onCancel = vi.fn();
    const { rerender } = render(
      <ConfirmDialog
        title="Delete website?"
        description="Cannot be undone."
        confirmLabel="Delete"
        cancelLabel="Cancel"
        busy={false}
        onConfirm={() => {}}
        onCancel={onCancel}
      />,
    );
    expect(screen.getByRole('button', { name: 'Delete' })).toHaveFocus();
    await userEvent.keyboard('{Escape}');
    expect(onCancel).toHaveBeenCalledOnce();
    onCancel.mockClear();
    rerender(
      <ConfirmDialog
        title="Delete website?"
        description="Cannot be undone."
        confirmLabel="Delete"
        cancelLabel="Cancel"
        busy
        onConfirm={() => {}}
        onCancel={onCancel}
      />,
    );
    await userEvent.keyboard('{Escape}');
    expect(onCancel).not.toHaveBeenCalled();
  });

  it('keeps compact layout, long-content, and reduced-motion contracts', () => {
    const css = readFileSync('src/app/app.css', 'utf8');
    expect(css).toContain('@media (max-width: 900px)');
    expect(css).toContain('@media (max-width: 680px)');
    expect(css).toContain('@media (prefers-reduced-motion: reduce)');
    expect(css).toContain('overflow-wrap: anywhere');
  });
});
