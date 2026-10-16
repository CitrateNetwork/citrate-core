// HUP-S10.6 — test-only accessibility harness (imported by *.a11y.test.tsx, never by app code).
//
// Renders a React element into a real jsdom document and runs axe-core over it. jsdom has no
// layout or computed CSS custom properties, so axe's colour-contrast rule cannot judge anything
// here; contrast is checked separately against the register tokens (contrast.a11y.test.ts).
import axe from "axe-core";
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

export type Mounted = { host: HTMLElement; rerender: (el: ReactElement) => void; unmount: () => void };

const mounted: Mounted[] = [];

/** Mount into document.body (focus and keyboard events need a connected node). */
export function mount(el: ReactElement): Mounted {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  act(() => root.render(el));
  const m: Mounted = {
    host,
    rerender: (next) => act(() => root.render(next)),
    unmount: () => {
      act(() => root.unmount());
      host.remove();
    },
  };
  mounted.push(m);
  return m;
}

/** Unmount everything mounted since the last cleanup (call from afterEach). */
export function cleanupMounted(): void {
  while (mounted.length) {
    const m = mounted.pop();
    try {
      m?.unmount();
    } catch {
      /* already unmounted by the test */
    }
  }
}

/** A short, readable line per violation: rule id, impact, and the offending node targets. */
export type AxeFinding = { id: string; impact: string | null | undefined; targets: string[] };

/** Run axe-core (WCAG 2.0/2.1/2.2 A + AA and best-practice tags) over `node`. */
export async function axeFindings(node: Element): Promise<AxeFinding[]> {
  const res = await axe.run(node, {
    runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"] },
    rules: {
      // jsdom cannot compute colours (no layout, no var() resolution): see contrast.a11y.test.ts.
      "color-contrast": { enabled: false },
      // Components are rendered as fragments of a page, not whole documents.
      "page-has-heading-one": { enabled: false },
      region: { enabled: false },
    },
  });
  return res.violations.map((v) => ({ id: v.id, impact: v.impact, targets: v.nodes.map((n) => String(n.target.join(" "))) }));
}

/** Dispatch a keydown on the focused element (or `target`), bubbling like a real key press. */
export function press(key: string, opts: { shift?: boolean; target?: Element | null } = {}): KeyboardEvent {
  const target = opts.target ?? document.activeElement ?? document.body;
  const ev = new KeyboardEvent("keydown", { key, shiftKey: !!opts.shift, bubbles: true, cancelable: true });
  act(() => {
    target.dispatchEvent(ev);
  });
  return ev;
}
