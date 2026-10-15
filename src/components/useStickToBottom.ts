// =====================================================================
// HGW (v0.5.1): the Hermes chat stays on the latest message.
//
// Before, the follow was an effect keyed on `chatMsgs` and re-armed only by send(): height
// changes from anything else (the thinking row, the local-model prompt card, Markdown reflow, the
// picker opening, a resize) left the end hidden, a workflow run / retry / skill run streamed with
// whatever follow was left over, and nothing brought a member who had scrolled up back.
//
// Now:
//   - Pinned by default. While pinned, ANY size change of the list or its content re-pins
//     (a ResizeObserver on both), plus a per-commit pin as a belt-and-braces path.
//   - Only the member detaches it: a scroll counts as theirs only right after their own upward
//     input on the list (wheel up, touch, an upward key, a pointer held on it), and only once it
//     leaves the bottom zone (40 px). A programmatic scroll, or the browser reporting a position
//     after content grew, never detaches. While that gesture lasts, the list is left to them.
//   - A scroll that lands in the bottom zone without upward input (their scroll down, a release
//     of the scrollbar) re-pins.
//   - Every new turn re-arms it, whatever started the turn (send, workflow run, retry, skill run,
//     a resumed session): Hermes going from ready to busy, or a new message of the member's own.
//   - While detached, "Jump to latest" shows, with the number of messages that arrived since.
// =====================================================================
import { useCallback, useLayoutEffect, useRef, useState, type KeyboardEvent, type UIEvent, type WheelEvent } from "react";
import { shouldFollow } from "./chatLayout";

export type StickState = { pinned: boolean; unseen: number };
export type StickEvent =
  | { type: "messages"; added: number }
  | { type: "scroll"; userIntent: boolean; atBottom: boolean }
  | { type: "turn" }
  | { type: "jump" };

export const STICK_INITIAL: StickState = { pinned: true, unseen: 0 };
const PINNED: StickState = STICK_INITIAL;

/** The follow state machine. Returns the same object when nothing changed. */
export function stickReducer(s: StickState, e: StickEvent): StickState {
  switch (e.type) {
    case "messages":
      if (s.pinned || e.added <= 0) return s;
      return { pinned: false, unseen: s.unseen + e.added };
    case "scroll":
      // While the member is moving toward older messages, only leaving the bottom zone counts.
      if (e.userIntent) return !e.atBottom && s.pinned ? { pinned: false, unseen: 0 } : s;
      // Otherwise (their downward scroll, or a programmatic one) reaching the bottom re-pins, and
      // a position away from the bottom changes nothing: content may simply have grown.
      if (e.atBottom) return s.pinned && s.unseen === 0 ? s : PINNED;
      return s;
    case "turn":
    case "jump":
      return s.pinned && s.unseen === 0 ? s : PINNED;
  }
}

export type TurnMark = { busy: boolean; lastOutgoingId: string | undefined };

/** A new turn began: Hermes went from ready to busy, or the member added a message of their own. */
export function isNewTurn(prev: TurnMark, next: TurnMark): boolean {
  if (!prev.busy && next.busy) return true;
  return next.lastOutgoingId !== undefined && next.lastOutgoingId !== prev.lastOutgoingId;
}

/** Keys that scroll a focused list toward older messages. */
export function isUpwardIntentKey(key: string, shift: boolean): boolean {
  return key === "ArrowUp" || key === "PageUp" || key === "Home" || (key === " " && shift);
}

/** Keys that scroll a focused list toward the latest message. */
export function isDownwardKey(key: string, shift: boolean): boolean {
  return key === "ArrowDown" || key === "PageDown" || key === "End" || (key === " " && !shift);
}

/** How long after the member's own input a scroll still counts as theirs. */
const INTENT_MS = 300;

function prefersReducedMotion(): boolean {
  try {
    return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch {
    return false;
  }
}

export type StickToBottom = {
  /** Callback ref for the scrolling list. */
  listRef: (el: HTMLElement | null) => void;
  /** Callback ref for the wrapper around the list's content (observed for size changes). */
  contentRef: (el: HTMLElement | null) => void;
  /** Spread onto the list: classifies scrolls as the member's or not. */
  listHandlers: {
    onScroll: (e: UIEvent<HTMLElement>) => void;
    onWheel: (e: WheelEvent<HTMLElement>) => void;
    onKeyDown: (e: KeyboardEvent<HTMLElement>) => void;
    onTouchStart: () => void;
    onTouchMove: () => void;
    onPointerDown: () => void;
  };
  pinned: boolean;
  unseen: number;
  /** Scroll to the latest message and follow again. */
  jumpToLatest: () => void;
  /** Re-arm the follow (a new turn) and go to the latest message. */
  rearm: () => void;
  /** Keep the end in view if (and only if) the follow is on. */
  follow: () => void;
};

export function useStickToBottom(opts: { messageCount: number; busy: boolean; lastOutgoingId: string | undefined }): StickToBottom {
  const state = useRef<StickState>(STICK_INITIAL);
  const [view, setView] = useState<StickState>(STICK_INITIAL);
  const list = useRef<HTMLElement | null>(null);
  const content = useRef<HTMLElement | null>(null);
  const ro = useRef<ResizeObserver | null>(null);
  const intentUntil = useRef(0);
  const pointerHeld = useRef(false);

  const dispatch = useCallback((e: StickEvent) => {
    const next = stickReducer(state.current, e);
    if (next === state.current) return;
    state.current = next;
    setView(next);
  }, []);

  const userActive = () => pointerHeld.current || Date.now() < intentUntil.current;

  const toBottom = useCallback((smooth = false) => {
    const el = list.current;
    if (!el) return;
    if (smooth && typeof el.scrollTo === "function" && !prefersReducedMotion()) el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
    else el.scrollTop = el.scrollHeight;
  }, []);

  const follow = useCallback(() => {
    // While the member is mid-gesture, leave the position to them; their scroll decides.
    if (!state.current.pinned || userActive()) return;
    toBottom();
  }, [toBottom]);

  const observer = useCallback((): ResizeObserver | null => {
    if (ro.current) return ro.current;
    if (typeof ResizeObserver === "undefined") return null;
    ro.current = new ResizeObserver(() => follow());
    return ro.current;
  }, [follow]);

  const listRef = useCallback(
    (el: HTMLElement | null) => {
      if (list.current === el) return;
      const o = observer();
      if (list.current) o?.unobserve(list.current);
      list.current = el;
      if (el) {
        o?.observe(el);
        follow();
      }
    },
    [observer, follow],
  );
  const contentRef = useCallback(
    (el: HTMLElement | null) => {
      if (content.current === el) return;
      const o = observer();
      if (content.current) o?.unobserve(content.current);
      content.current = el;
      if (el) o?.observe(el);
    },
    [observer],
  );
  useLayoutEffect(
    () => () => {
      ro.current?.disconnect();
      ro.current = null;
    },
    [],
  );

  // A pointer held on the list (a scrollbar drag, a text selection) is the member's, until release.
  useLayoutEffect(() => {
    const release = () => {
      if (!pointerHeld.current) return;
      pointerHeld.current = false;
      intentUntil.current = 0;
      // Let go at the bottom (a scrollbar dragged all the way down): follow again.
      const el = list.current;
      if (el) dispatch({ type: "scroll", userIntent: false, atBottom: shouldFollow(el) });
    };
    window.addEventListener("pointerup", release);
    window.addEventListener("pointercancel", release);
    return () => {
      window.removeEventListener("pointerup", release);
      window.removeEventListener("pointercancel", release);
    };
  }, [dispatch]);

  // New turns re-arm; messages that arrive while reading back are counted. Runs before paint.
  const prevTurn = useRef<TurnMark>({ busy: opts.busy, lastOutgoingId: opts.lastOutgoingId });
  const prevCount = useRef(opts.messageCount);
  useLayoutEffect(() => {
    const mark: TurnMark = { busy: opts.busy, lastOutgoingId: opts.lastOutgoingId };
    if (isNewTurn(prevTurn.current, mark)) {
      intentUntil.current = 0;
      dispatch({ type: "turn" });
    }
    prevTurn.current = mark;
    const added = opts.messageCount - prevCount.current;
    prevCount.current = opts.messageCount;
    if (added > 0) dispatch({ type: "messages", added });
  }, [opts.busy, opts.lastOutgoingId, opts.messageCount, dispatch]);

  // Every commit: the per-render pin (also covers hosts without a ResizeObserver).
  useLayoutEffect(() => {
    follow();
  });

  const markIntent = () => {
    intentUntil.current = Date.now() + INTENT_MS;
  };
  // The latest gesture's direction wins: heading down ends an upward gesture.
  const clearIntent = () => {
    intentUntil.current = 0;
  };

  const listHandlers: StickToBottom["listHandlers"] = {
    onScroll: (e) => dispatch({ type: "scroll", userIntent: userActive(), atBottom: shouldFollow(e.currentTarget) }),
    onWheel: (e) => {
      if (e.deltaY < 0) markIntent();
      else if (e.deltaY > 0) clearIntent();
    },
    onKeyDown: (e) => {
      if (isUpwardIntentKey(e.key, e.shiftKey)) markIntent();
      else if (isDownwardKey(e.key, e.shiftKey)) clearIntent();
    },
    onTouchStart: markIntent,
    onTouchMove: markIntent,
    onPointerDown: () => {
      pointerHeld.current = true;
    },
  };

  const jumpToLatest = useCallback(() => {
    intentUntil.current = 0;
    pointerHeld.current = false;
    dispatch({ type: "jump" });
    toBottom(true);
  }, [dispatch, toBottom]);

  const rearm = useCallback(() => {
    intentUntil.current = 0;
    dispatch({ type: "turn" });
    toBottom();
  }, [dispatch, toBottom]);

  return { listRef, contentRef, listHandlers, pinned: view.pinned, unseen: view.unseen, jumpToLatest, rearm, follow };
}
