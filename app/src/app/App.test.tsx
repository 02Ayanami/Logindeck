import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { App } from './App';

describe('App', () => {
  it('renders the LoginDeck shell and opens Settings', async () => {
    render(<App />);
    expect(screen.getByText('LoginDeck')).toBeVisible();
    expect(screen.getByRole('link', { name: 'Website accounts' })).toBeVisible();
    expect(screen.getByRole('link', { name: 'Application accounts' })).toBeVisible();
    await userEvent.click(screen.getByRole('link', { name: 'Settings' }));
    expect(screen.getByRole('heading', { name: 'Settings' })).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Language' })).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Edge login detection' })).toBeVisible();
  });
});
