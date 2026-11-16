// =====================================================================
// citrate-core: the escrow bar (FL design board 12, "Escrow bar")
//
// A round's budget split into paid, reserved for accepted units, and refundable. SALT only. The
// parts differ by fill (solid, striped, empty) and a legend names each, so colour is never alone.
// Widths move on WorkMarket UnitsReserved / Settled / Refunded (the caller passes new amounts).
// Unread: the bar is hidden and the reason shown with "—". Source: WorkMarket.getOffer(id).
// =====================================================================
import { saltFromWei } from "./format";
import { Unknown } from "./primitives";

export interface Escrow {
  budgetWei: bigint;
  paidWei: bigint;
  reservedWei: bigint;
  refundableWei: bigint;
}

export function escrowSentence(e: Escrow): string {
  return `Escrow of ${saltFromWei(e.budgetWei)} SALT: ${saltFromWei(e.paidWei)} paid, ${saltFromWei(e.reservedWei)} reserved, ${saltFromWei(e.refundableWei)} refundable`;
}

const PARTS = [
  { k: "paidWei", word: "paid", color: "var(--fl-escrow-paid)", fill: "solid" },
  { k: "reservedWei", word: "reserved", color: "var(--fl-escrow-reserved)", fill: "striped" },
  { k: "refundableWei", word: "refundable", color: "var(--fl-escrow-refundable)", fill: "empty" },
] as const;

export function EscrowBar({ escrow, reason }: { escrow: Escrow | null; reason?: string }) {
  if (!escrow) {
    return (
      <div data-testid="escrow-bar" data-read="false" style={{ fontSize: 13 }}>
        Escrow: <Unknown reason={reason ?? "the offer was not read"} />
      </div>
    );
  }
  const sentence = escrowSentence(escrow);
  const total = escrow.budgetWei > 0n ? escrow.budgetWei : 1n;
  const pct = (w: bigint) => Number((w * 10000n) / total) / 100;
  return (
    <div data-testid="escrow-bar" data-read="true" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <span style={{ fontSize: 13 }}>{sentence}</span>
      <div role="img" aria-label={sentence} style={{ display: "flex", height: 12, borderRadius: 3, overflow: "hidden", border: "1px solid var(--line-2)" }}>
        {PARTS.map((p) => (
          <div
            key={p.k}
            className={"fl-escrow-seg" + (p.fill === "striped" ? " fl-striped" : "")}
            data-part={p.word}
            style={{ flexGrow: pct(escrow[p.k]), flexBasis: 0, background: p.fill === "solid" ? p.color : p.fill === "empty" ? "transparent" : undefined, color: p.color }}
          />
        ))}
      </div>
      <ul aria-hidden="true" style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", gap: 14, flexWrap: "wrap", fontSize: 11.5 }}>
        {PARTS.map((p) => (
          <li key={p.k} style={{ display: "flex", gap: 6, alignItems: "center" }}>
            <span className={p.fill === "striped" ? "fl-striped" : undefined} style={{ width: 12, height: 8, border: "1px solid " + p.color, background: p.fill === "solid" ? p.color : undefined, color: p.color }} />
            <span className="fl-muted">{p.word}</span>
            <span className="mono">{saltFromWei(escrow[p.k])} SALT</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
