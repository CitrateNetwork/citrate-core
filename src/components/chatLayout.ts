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

// 0.5.4 — the composer grows with its content (a large paste shows), up to a cap, then scrolls.

/** One line of the composer (the old single-line input's height). */
export const COMPOSER_MIN_HEIGHT = 36;
/** The tallest the composer grows before it scrolls inside itself. */
export const COMPOSER_MAX_HEIGHT = 240;
/** The composer never takes more than this share of the pane, so the transcript stays visible. */
export const COMPOSER_MAX_PANE_SHARE = 0.4;

/** The composer's cap for a pane of `paneHeight` px. */
export function composerMax(paneHeight: number): number {
  const share = Math.floor(paneHeight * COMPOSER_MAX_PANE_SHARE);
  return Math.max(COMPOSER_MIN_HEIGHT, Math.min(COMPOSER_MAX_HEIGHT, Number.isFinite(share) ? share : COMPOSER_MAX_HEIGHT));
}

/** The composer's height for content `scrollHeight` px tall, and whether it scrolls inside. */
export function composerHeight(scrollHeight: number, max: number = COMPOSER_MAX_HEIGHT, min: number = COMPOSER_MIN_HEIGHT): { height: number; scroll: boolean } {
  const content = Number.isFinite(scrollHeight) ? Math.ceil(scrollHeight) : 0;
  if (content > max) return { height: max, scroll: true };
  return { height: Math.max(min, content), scroll: false };
}

/** Size a composer text area to its content (call after every change to its value). */
export function fitComposer(el: HTMLTextAreaElement | null, max: number = COMPOSER_MAX_HEIGHT): void {
  if (!el) return;
  // Collapse first so scrollHeight measures the content, not the previous height.
  el.style.height = "auto";
  // scrollHeight leaves out the borders; a border-box height has to include them.
  const cs = typeof getComputedStyle === "function" ? getComputedStyle(el) : null;
  const borders = cs ? (parseFloat(cs.borderTopWidth) || 0) + (parseFloat(cs.borderBottomWidth) || 0) : 0;
  const { height, scroll } = composerHeight(el.scrollHeight + borders, max);
  el.style.height = height + "px";
  el.style.overflowY = scroll ? "auto" : "hidden";
}
