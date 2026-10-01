// =====================================================================
// citrate-core — accessible modal frame for the approval dialogs (HUP-S10.6)
//
// The SignatureCeremony and the wallet review render inside this frame. It provides what a
// keyboard or screen-reader member needs from a modal:
//   • role="dialog" + aria-modal, named and described by ids the caller passes;
//   • initial focus on the element marked data-autofocus (the SAFE choice: Decline / Reject;
//     Approve is never default-focused, T2), else the dialog itself, and only when `focusKey`
//     changes, so a re-render (ticking a checkbox, a queue update) never steals focus;
//   • Tab / Shift+Tab wrap inside the dialog, and focus that lands outside is pulled back in;
//   • Escape calls `onEscape` when the caller allows it (only ever a decline, never an approval);
//   • on close, focus returns to the element that had it before the dialog opened.
// When two dialogs are open at once, only the most recently opened one traps focus.
// =====================================================================
import { useEffect, useRef, type CSSProperties, type KeyboardEvent, type ReactNode } from "react";

const FOCUSABLE = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled]):not([type='hidden'])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

/** The keyboard-reachable elements inside `root`, in document order. */
export function focusablesIn(root: HTMLElement): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => !el.closest("[inert]") && el.getAttribute("aria-hidden") !== "true");
}

// Open dialogs, most recent last. Only the top one traps focus.
const openDialogs: HTMLElement[] = [];

export type ModalDialogProps = {
  /** id of the element that names the dialog (its heading). */
  labelledBy?: string;
  /** Fallback accessible name when there is no visible heading. */
  label?: string;
  /** id of the element that describes the dialog. */
  describedBy?: string;
  /** Called on Escape; omit to make Escape do nothing (for example while a signature is in flight). */
  onEscape?: () => void;
  /** Initial focus is (re)applied only when this value changes. */
  focusKey: string;
  register: "charter" | "instrument";
  zIndex: number;
  panelStyle: CSSProperties;
  children: ReactNode;
};

export function ModalDialog({ labelledBy, label, describedBy, onEscape, focusKey, register, zIndex, panelStyle, children }: ModalDialogProps) {
  const panel = useRef<HTMLDivElement | null>(null);

  // Register as the top dialog; restore focus to the opener when this dialog closes.
  useEffect(() => {
    const el = panel.current;
    if (!el) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    openDialogs.push(el);
    const onFocusIn = (e: FocusEvent) => {
      if (openDialogs[openDialogs.length - 1] !== el) return;
      const t = e.target;
      if (t instanceof Node && el.contains(t)) return;
      const first = focusablesIn(el)[0] ?? el;
      first.focus();
    };
    document.addEventListener("focusin", onFocusIn);
    return () => {
      document.removeEventListener("focusin", onFocusIn);
      const i = openDialogs.lastIndexOf(el);
      if (i >= 0) openDialogs.splice(i, 1);
      if (opener && opener.isConnected) opener.focus();
    };
  }, []);

  // Initial focus: the safe choice, once per focusKey.
  useEffect(() => {
    const el = panel.current;
    if (!el) return;
    const target = el.querySelector<HTMLElement>("[data-autofocus]") ?? el;
    target.focus();
  }, [focusKey]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const el = panel.current;
    if (!el) return;
    if (e.key === "Escape") {
      if (onEscape) {
        e.preventDefault();
        e.stopPropagation();
        onEscape();
      }
      return;
    }
    if (e.key !== "Tab") return;
    const items = focusablesIn(el);
    if (items.length === 0) {
      e.preventDefault();
      el.focus();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    if (e.shiftKey && (active === first || active === el)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && (active === last || active === el)) {
      e.preventDefault();
      first.focus();
    }
  };

  return (
    <div data-register={register} style={{ position: "fixed", inset: 0, background: "rgba(14,15,12,.44)", display: "flex", alignItems: "center", justifyContent: "center", zIndex, padding: 24 }}>
      <div
        ref={panel}
        className="cc-fade-up"
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
        aria-label={labelledBy ? undefined : label}
        aria-describedby={describedBy}
        tabIndex={-1}
        onKeyDown={onKeyDown}
        style={{ outline: "none", ...panelStyle }}
      >
        {children}
      </div>
    </div>
  );
}
