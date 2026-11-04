// CORE-D3 — the member-facing line for a `sidecars://alert` (Rust `services.rs` alert watch): a
// sidecar spent its retry budget, or came back from it. Every word comes from the alert.
import type { SidecarAlert } from "../bridge/domains";

/** "in N s" / "in N min" / "in N h" from `from` to `at`. */
export function untilText(from: number, at: number): string {
  const s = Math.max(0, Math.round((at - from) / 1000));
  if (s < 60) return `in ${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `in ${m} min`;
  return `in ${Math.floor(m / 60)} h`;
}

export function sidecarAlertText(a: SidecarAlert): string {
  if (a.kind === "recovered") return `${a.label} is running again.`;
  const why = a.reason ? ` (${a.reason})` : "";
  const next = a.nextRetryAtUnixMs !== null ? ` It is tried again ${untilText(a.atUnixMs, a.nextRetryAtUnixMs)}.` : "";
  return `${a.label} failed${why}.${next}`;
}
