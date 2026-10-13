// v0.5.0-rc.5 FIX 4 — the chat pane fits the window.
//
// The pane's height is the space between its top edge (as it sits with the page scrolled to the
// top) and the bottom of the visible scroll area, so the composer is on screen without
// scrolling, the pane grows with the window, and the page scroll carries everything else. Only
// the message list scrolls inside the pane.
import { useLayoutEffect, useState, type RefObject } from "react";

/** The smallest the pane gets, so a tiny window still has a usable transcript + composer. */
export const CHAT_MIN_HEIGHT = 320;
/** The page's bottom padding below the pane. */
export const CHAT_BOTTOM_GAP = 24;

export function fitChatHeight(m: { viewportBottom: number; chatTop: number; scrollTop: number; min?: number; gap?: number }): number {
  const min = m.min ?? CHAT_MIN_HEIGHT;
  const gap = m.gap ?? CHAT_BOTTOM_GAP;
  const avail = Math.floor(m.viewportBottom - (m.chatTop + m.scrollTop) - gap);
  return Number.isFinite(avail) ? Math.max(min, avail) : min;
}

/** Follow new messages only while the member is at (or within ~40 px of) the bottom. */
export function shouldFollow(el: { scrollHeight: number; scrollTop: number; clientHeight: number }): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight < 40;
}

function scrollParent(el: HTMLElement): HTMLElement | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const o = (p.style.overflowY || p.style.overflow || getComputedStyle(p).overflowY || "").toLowerCase();
    if (o === "auto" || o === "scroll") return p;
  }
  return null;
}

function measure(el: HTMLElement): number {
  const parent = scrollParent(el);
  const top = el.getBoundingClientRect().top;
  if (parent) {
    const pr = parent.getBoundingClientRect();
    return fitChatHeight({ viewportBottom: Math.min(pr.bottom, window.innerHeight), chatTop: top, scrollTop: parent.scrollTop });
  }
  return fitChatHeight({ viewportBottom: window.innerHeight, chatTop: top, scrollTop: window.scrollY || 0 });
}

/** The fitted height for the element behind `ref`; re-measured on every render and on resize. */
export function useFitToViewport(ref: RefObject<HTMLElement | null>): number {
  const [h, setH] = useState(CHAT_MIN_HEIGHT);
  // Every commit: content above the pane (a banner, the vitals strip) can move its top edge.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const next = measure(el);
    if (next !== h) setH(next);
  });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const update = () => setH(measure(el));
    window.addEventListener("resize", update);
    const parent = scrollParent(el);
    const ro = typeof ResizeObserver !== "undefined" && parent ? new ResizeObserver(update) : null;
    if (ro && parent) ro.observe(parent);
    return () => {
      window.removeEventListener("resize", update);
      ro?.disconnect();
    };
  }, [ref]);
  return h;
}
