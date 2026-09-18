import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { App } from './App';

describe('App', () => {
  it('renders the LoginDeck shell and its destinations', () => {
    render(<App />);
    expect(screen.getByText('LoginDeck')).toBeVisible();
    expect(screen.getByRole('link', { name: 'Website accounts' })).toBeVisible();
    expect(screen.getByRole('link', { name: 'Application accounts' })).toBeVisible();
    expect(screen.queryByRole('link', { name: 'Settings' })).toBeNull();
  });
});
