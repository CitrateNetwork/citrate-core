// 0.5.4 — the keyboard shortcuts for the two collapsible columns, as in most desktop editors:
//   ⌘B / Ctrl+B          the sidebar
//   ⌥⌘B / Ctrl+Alt+B     the Hermes chat's right column (widgets, daemons, transactions, tutorials)
// The modifier is ⌘ on macOS and Ctrl elsewhere (Ctrl+B on macOS is a text-field cursor move, so it
// is left alone there). The match is on the physical key (`code`), because ⌥B types "∫" on a Mac.
import { useEffect } from "react";

export type LayoutTarget = "sidebar" | "rail";

type KeyLike = Pick<KeyboardEvent, "code" | "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">;

export function isMacPlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent || "");
}

/** Which column a key press toggles, or null. Pure: the platform is passed in. */
export function layoutShortcut(e: KeyLike, mac: boolean): LayoutTarget | null {
  const isB = e.code === "KeyB" || e.key === "b" || e.key === "B";
  if (!isB || e.shiftKey) return null;
  const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  if (!mod) return null;
  return e.altKey ? "rail" : "sidebar";
}

/** The label a tooltip shows for a column's shortcut on this platform. */
export function shortcutLabel(target: LayoutTarget, mac: boolean = isMacPlatform()): string {
  if (mac) return target === "rail" ? "⌥⌘B" : "⌘B";
  return target === "rail" ? "Ctrl+Alt+B" : "Ctrl+B";
}

/** Calls `onToggle` when the shortcut for `target` is pressed anywhere in the window. */
export function useLayoutShortcut(target: LayoutTarget, onToggle: () => void): void {
  useEffect(() => {
    const mac = isMacPlatform();
    const onKey = (e: KeyboardEvent) => {
      if (e.repeat || layoutShortcut(e, mac) !== target) return;
      e.preventDefault();
      onToggle();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [target, onToggle]);
}
