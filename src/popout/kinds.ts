// =====================================================================
// citrate-core — pop-out kinds (HUP-S5.4, D-36)
//
// The closed allowlist of pop-out windows. It is mirrored 1:1 by Rust `popout.rs` (`PopoutKind`)
// and by the window list of `src-tauri/capabilities/popout.json`; the Rust and capability tests
// keep the three in step. A window label that is not `popout-<kind>` for a kind on this list is
// never treated as a pop-out.
// =====================================================================

export const POPOUT_KINDS = ["browser", "contract", "monitor", "diff", "media"] as const;
export type PopoutKind = (typeof POPOUT_KINDS)[number];

export const POPOUT_LABEL_PREFIX = "popout-";

export const POPOUT_TITLES: Record<PopoutKind, string> = {
  browser: "Browser",
  contract: "Contract reader",
  monitor: "Activity monitor",
  diff: "Code and diff",
  media: "Media player",
};

export function isPopoutKind(v: unknown): v is PopoutKind {
  return typeof v === "string" && (POPOUT_KINDS as readonly string[]).includes(v);
}

export function popoutLabel(kind: PopoutKind): string {
  return POPOUT_LABEL_PREFIX + kind;
}

/** The pop-out kind a window label names, or null for the main window and anything unknown. */
export function popoutKindFromLabel(label: string): PopoutKind | null {
  if (!label.startsWith(POPOUT_LABEL_PREFIX)) return null;
  const kind = label.slice(POPOUT_LABEL_PREFIX.length);
  return isPopoutKind(kind) ? kind : null;
}
