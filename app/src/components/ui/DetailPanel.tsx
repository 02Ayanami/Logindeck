import {
  type ReactNode,
  type RefObject,
  useId,
  useLayoutEffect,
  useRef,
} from 'react';

export type DetailPanelProps = {
  title: string;
  open: boolean;
  closeLabel: string;
  onClose: () => void;
  returnFocusRef: RefObject<HTMLElement | null>;
  children: ReactNode;
  footer?: ReactNode;
};

export function DetailPanel({
  title,
  open,
  closeLabel,
  onClose,
  returnFocusRef,
  children,
  footer,
}: DetailPanelProps) {
  const titleId = useId();
  const titleRef = useRef<HTMLHeadingElement>(null);
  const wasOpen = useRef(false);

  useLayoutEffect(() => {
    if (open) {
      wasOpen.current = true;
      titleRef.current?.focus();
      return;
    }

    if (wasOpen.current) {
      wasOpen.current = false;
      returnFocusRef.current?.focus();
    }
  }, [open, returnFocusRef]);

  useLayoutEffect(() => () => {
    if (wasOpen.current) returnFocusRef.current?.focus();
  }, [returnFocusRef]);

  if (!open) return null;

  return (
    <div
      className="detail-panel__backdrop"
      onKeyDown={(event) => {
        if (event.key === 'Escape') onClose();
      }}
    >
      <section className="detail-panel" data-detail-panel aria-labelledby={titleId}>
        <header className="detail-panel__header">
          <h2 id={titleId} ref={titleRef} tabIndex={-1}>{title}</h2>
          <button className="detail-panel__close" type="button" aria-label={closeLabel} title={closeLabel} onClick={onClose}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg>
          </button>
        </header>
        <div className="detail-panel__body">{children}</div>
        {footer ? <footer className="detail-panel__footer">{footer}</footer> : null}
      </section>
    </div>
  );
}
