import type { ReactNode } from 'react';

export type NoticeProps = {
  kind: 'success' | 'info' | 'error';
  children: ReactNode;
};

export function Notice({ kind, children }: NoticeProps) {
  return (
    <div className={`notice notice--${kind}`} role={kind === 'error' ? 'alert' : 'status'}>
      {children}
    </div>
  );
}

export type EmptyStateProps = {
  title: string;
  description: string;
  action?: ReactNode;
};

export function EmptyState({ title, description, action }: EmptyStateProps) {
  return (
    <div className="empty-state">
      <div className="empty-state__illustration" aria-hidden="true">
        <span />
      </div>
      <h2>{title}</h2>
      <p>{description}</p>
      {action ? <div className="empty-state__action">{action}</div> : null}
    </div>
  );
}

export type SkeletonListProps = {
  label: string;
  rows: number;
};

export function SkeletonList({ label, rows }: SkeletonListProps) {
  return (
    <div className="skeleton-list" role="status" aria-label={label}>
      <span className="visually-hidden">{label}</span>
      {Array.from({ length: rows }, (_, index) => (
        <div className="skeleton-row" data-testid="skeleton-row" aria-hidden="true" key={index}>
          <span className="skeleton-row__icon" />
          <span className="skeleton-row__content">
            <span />
            <span />
          </span>
        </div>
      ))}
    </div>
  );
}
