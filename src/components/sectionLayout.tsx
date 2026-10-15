// =====================================================================
// HGW (v0.5.1): columned sections own their scroll.
//
// The Shell wraps every surface in one scroll container. For the Hermes section (the Dashboard,
// "Hermes" in the sidebar, and the Agent surface) that made the whole window scroll and carried
// the chat off screen. These routes now fill the Shell's content box exactly: the container does
// not scroll, the surface is a viewport-height flex column, its columns sit in a grid with one
// bounded row, and every column scrolls on its own with `overscroll-behavior: contain`, so a
// gesture at the end of one column never moves another. Every other surface keeps its page scroll.
//
// Below NARROW_SECTION_PX of section width the columns collapse to one, behind a small switch
// (never a stacked second row, which would bring the page scroll back).
// =====================================================================
import { useLayoutEffect, useState, type CSSProperties, type RefObject } from "react";

/** Routes whose surface fills the content box and scrolls per column. */
export const FILL_ROUTES: ReadonlySet<string> = new Set(["dashboard", "agent"]);

/** The Shell's surface container for a route. */
export function surfaceContainerStyle(route: string): CSSProperties {
  if (FILL_ROUTES.has(route)) return { flex: 1, minHeight: 0, overflow: "hidden", display: "flex", flexDirection: "column" };
  return { flex: 1, minHeight: 0, overflow: "auto" };
}

/** A fill surface's root: exactly the section's height, never taller. */
export const SECTION_ROOT: CSSProperties = {
  flex: 1,
  height: "100%",
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  gap: 16,
  overflow: "hidden",
  boxSizing: "border-box",
  padding: "20px 26px 24px",
};

/** The column grid: one row that takes the remaining height; columns stretch to it. */
export function columnGrid(columns: string): CSSProperties {
  return {
    flex: 1,
    minHeight: 0,
    display: "grid",
    gridTemplateColumns: columns,
    gridTemplateRows: "minmax(0, 1fr)",
    alignItems: "stretch",
    gap: 16,
  };
}

/** A column that scrolls itself. A grid, not a flex column, so panels keep their natural height
 *  and overflow (a flex column would shrink and clip them). */
export const SCROLL_COLUMN: CSSProperties = {
  minHeight: 0,
  minWidth: 0,
  overflow: "auto",
  overscrollBehavior: "contain",
  display: "grid",
  gridTemplateColumns: "minmax(0, 1fr)",
  alignContent: "start",
  gap: 16,
};

/** A column whose child (the chat pane) manages its own scroll. */
export const CHAT_COLUMN: CSSProperties = {
  minHeight: 0,
  minWidth: 0,
  overflow: "hidden",
  display: "flex",
  flexDirection: "column",
};

/** Section width below which the columns collapse to one (about a 980 px window with the sidebar). */
export const NARROW_SECTION_PX = 760;

/** Whether the section behind `ref` (marked `data-section`) is narrower than the breakpoint. */
export function useNarrow(ref: RefObject<HTMLElement | null>, px: number = NARROW_SECTION_PX): boolean {
  const [narrow, setNarrow] = useState(false);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => {
      const w = el.clientWidth;
      // No layout (or not yet laid out): keep the wide layout rather than guess.
      setNarrow(w > 0 && w < px);
    };
    measure();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", measure);
      return () => window.removeEventListener("resize", measure);
    }
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref, px]);
  return narrow;
}

export type SwitchOption<T extends string> = { id: T; label: string; badge?: number };

/** The one-column switch shown on narrow windows. */
export function SectionSwitch<T extends string>({ options, value, onChange, label }: { options: SwitchOption<T>[]; value: T; onChange: (v: T) => void; label: string }) {
  return (
    <div
      role="tablist"
      aria-label={label}
      data-testid="section-switch"
      style={{ display: "flex", gap: 2, alignSelf: "flex-start", background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: 2, flexShrink: 0 }}
    >
      {options.map((o) => {
        const on = o.id === value;
        return (
          <button
            key={o.id}
            role="tab"
            aria-selected={on}
            onClick={() => onChange(o.id)}
            style={{ fontFamily: "var(--font-sans)", fontSize: 12, fontWeight: on ? 500 : 400, padding: "5px 12px", border: "none", borderRadius: 5, cursor: "pointer", background: on ? "var(--srf-2)" : "transparent", color: on ? "var(--tx-1)" : "var(--tx-2)", display: "inline-flex", alignItems: "center", gap: 6 }}
          >
            {o.label}
            {o.badge ? (
              <span className="mono tabular" style={{ fontSize: 10, minWidth: 16, height: 16, padding: "0 4px", borderRadius: 999, background: "var(--citrate-yellow)", color: "var(--on-highlight)", display: "inline-flex", alignItems: "center", justifyContent: "center", fontWeight: 600 }}>
                {o.badge}
              </span>
            ) : null}
          </button>
        );
      })}
    </div>
  );
}
