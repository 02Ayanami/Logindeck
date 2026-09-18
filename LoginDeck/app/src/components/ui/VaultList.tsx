import type { ReactNode } from 'react';

export type VaultListProps = {
  label: string;
  children: ReactNode;
};

export function VaultList({ label, children }: VaultListProps) {
  return (
    <ul className="vault-list" aria-label={label}>
      {children}
    </ul>
  );
}

export type VaultRowProps = {
  selected: boolean;
  children: ReactNode;
};

export function VaultRow({ selected, children }: VaultRowProps) {
  return (
    <li className="vault-row" data-selected={selected}>
      {children}
    </li>
  );
}
