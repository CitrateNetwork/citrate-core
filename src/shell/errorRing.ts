// Telemetry WP-T.2 (frontend half) — a tiny in-memory ring of the most recent UI errors,
// fed by the top-level ErrorBoundary. Purely local; it feeds the diagnostics bundle only when
// the member explicitly builds/sends a report (WP-T.4). Nothing here egresses (WP-T.1).
//
// Module-level (not store state) so a render crash — which is exactly when the store may be
// mid-update — can still record without depending on React state. Capped + newest-last.

const MAX = 20;
const ring: string[] = [];

/** Record a UI error (message + optional component stack). Capped ring, newest-last. */
export function recordUiError(message: string): void {
  const m = (message || "").trim();
  if (!m) return;
  ring.push(m.length > 4000 ? m.slice(0, 4000) + "…" : m);
  while (ring.length > MAX) ring.shift();
}

/** The recorded UI errors (a copy), oldest→newest. Read when building a diagnostic bundle. */
export function getUiErrors(): string[] {
  return ring.slice();
}

/** Clear the ring (after a report is sent, or on demand). */
export function clearUiErrors(): void {
  ring.length = 0;
}
