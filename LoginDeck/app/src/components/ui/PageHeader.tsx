import type { ReactNode } from 'react';

export type PageHeaderProps = {
  title: string;
  description: string;
  search?: ReactNode;
  actions?: ReactNode;
};

export function PageHeader({ title, description, search, actions }: PageHeaderProps) {
  return (
    <header className="page-header">
      <div className="page-header__intro">
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      {search ? <div className="page-header__search">{search}</div> : null}
      {actions ? <div className="page-header__actions">{actions}</div> : null}
    </header>
  );
}
