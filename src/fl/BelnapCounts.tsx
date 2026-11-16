// =====================================================================
// citrate-core: Belnap counts (FL design board 12, "Belnap counts")
//
// Four labelled mono counts from the committed aggregate: true, false, both, neither. Unread
// shows "—" for each with the reason, never 0. Counts appear at once on MergeCommitted (no
// count-up). Source: the FederatedRoundLedger commit's Belnap aggregate.
// =====================================================================
import { UNKNOWN_GLYPH } from "./primitives";

export interface Belnap {
  t: number;
  f: number;
  both: number;
  neither: number;
}

const CELLS: [keyof Belnap, string][] = [
  ["t", "true"],
  ["f", "false"],
  ["both", "both"],
  ["neither", "neither"],
];

export function belnapSentence(b: Belnap | null, reason?: string): string {
  if (!b) return `Belnap result not read${reason ? ": " + reason : ""}`;
  return `Belnap result: true ${b.t}, false ${b.f}, both ${b.both}, neither ${b.neither}`;
}

export function BelnapCounts({ counts, reason }: { counts: Belnap | null; reason?: string }) {
  return (
    <div data-testid="belnap-counts" data-read={counts ? "true" : "false"} style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <dl role="img" aria-label={belnapSentence(counts, reason)} style={{ display: "grid", gridTemplateColumns: "repeat(4, minmax(0, 1fr))", gap: 6, margin: 0 }}>
        {CELLS.map(([k, word]) => (
          <div key={k} className="fl-inset" style={{ padding: "8px 10px", display: "flex", flexDirection: "column", gap: 2 }}>
            <dt className="eyebrow" style={{ fontSize: 10 }}>{word}</dt>
            <dd className="mono tabular" style={{ margin: 0, fontSize: 18, color: "var(--tx-1)" }}>{counts ? counts[k] : UNKNOWN_GLYPH}</dd>
          </div>
        ))}
      </dl>
      {!counts && reason && <span className="fl-faint" style={{ fontSize: 11.5 }}>{reason}</span>}
    </div>
  );
}
