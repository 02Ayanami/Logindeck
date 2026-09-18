import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import '../../app/i18n';
import { AppShell } from './AppShell';

describe('AppShell', () => {
  it('shows the LoginDeck brand and marks the active destination', () => {
    render(
      <AppShell page="websites" onNavigate={vi.fn()}>
        <p>content</p>
      </AppShell>,
    );

    expect(screen.getByText('LoginDeck')).toBeVisible();
    expect(screen.getByRole('link', { name: 'Website accounts' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.getByText('Passwords stay on this device')).toBeVisible();
    expect(screen.queryByRole('link', { name: 'Settings' })).toBeNull();
    expect(screen.getByText('content').closest('[data-layout]')).toHaveAttribute(
      'data-layout',
      'desktop',
    );
  });

  it('navigates without exposing navigation as feature logic', async () => {
    const onNavigate = vi.fn();
    render(
      <AppShell page="websites" onNavigate={onNavigate}>
        <p>content</p>
      </AppShell>,
    );

    await userEvent.click(screen.getByRole('link', { name: 'Application accounts' }));
    expect(onNavigate).toHaveBeenCalledWith('applications');
  });
});
