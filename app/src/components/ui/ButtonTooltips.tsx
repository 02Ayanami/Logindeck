import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';

/** Shared by page buttons and buttons rendered in portal drawers. */
export function ButtonTooltips() {
  const [tip, setTip] = useState<{ text: string; left: number; top: number; above: boolean }>();
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let current: HTMLElement | undefined;
    const hide = () => { clearTimeout(timer); current = undefined; setTip(undefined); };
    const show = (event: Event) => {
      if (event instanceof PointerEvent && event.pointerType === 'touch') return;
      const button = event.target instanceof Element ? event.target.closest<HTMLElement>('button, [role="button"], nav a') : null;
      if (!button) { hide(); return; }
      if (current === button) return;
      hide(); current = button;
      timer = setTimeout(() => {
        if (!button.isConnected) return;
        const text = button.getAttribute('aria-label') || button.textContent?.trim();
        if (!text) return;
        const rect = button.getBoundingClientRect();
        const above = rect.top > 64;
        setTip({ text, left: Math.max(120, Math.min(window.innerWidth - 120, rect.left + rect.width / 2)), top: above ? rect.top - 8 : rect.bottom + 8, above });
      }, 300);
    };
    const leave = (event: Event) => {
      const next = (event as MouseEvent).relatedTarget;
      if (next instanceof Node && current?.contains(next)) return;
      hide();
    };
    document.addEventListener('pointerover', show);
    document.addEventListener('pointerout', leave);
    document.addEventListener('focusin', show);
    document.addEventListener('focusout', hide);
    document.addEventListener('pointerdown', hide);
    document.addEventListener('keydown', hide);
    document.addEventListener('scroll', hide, true);
    window.addEventListener('resize', hide);
    return () => {
      clearTimeout(timer);
      document.removeEventListener('pointerover', show);
      document.removeEventListener('pointerout', leave);
      document.removeEventListener('focusin', show);
      document.removeEventListener('focusout', hide);
      document.removeEventListener('pointerdown', hide);
      document.removeEventListener('keydown', hide);
      document.removeEventListener('scroll', hide, true);
      window.removeEventListener('resize', hide);
    };
  }, []);
  return tip ? createPortal(<div className="button-tooltip" role="tooltip" style={{ left: tip.left, top: tip.top, transform: `translate(-50%, ${tip.above ? '-100%' : '0'})` }}>{tip.text}</div>, document.body) : null;
}
