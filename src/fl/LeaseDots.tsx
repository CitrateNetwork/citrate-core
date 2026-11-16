// =====================================================================
// citrate-core: lease dots (FL design board 12, "Lease dots")
//
// One glyph per lease, plus a selectable list. Your lease is first. Shape differs per state
// (○ leased, ● submitted, ✓ verified, ⊘ rejected), so colour is never alone. Source: the
// coordinator's /v1/status, labelled coordinator-reported by the caller. Only a changed lease
// re-colours (the glyph's colour transition, off under reduced motion).
// =====================================================================
export type LeaseState = "leased" | "submitted" | "verified" | "rejected";

export interface Lease {
  id: string;
  state: LeaseState;
  mine?: boolean;
  /** Why a rejected lease was rejected, in plain words. */
  reason?: string;
}

export const LEASE_GLYPH: Record<LeaseState, string> = { leased: "○", submitted: "●", verified: "✓", rejected: "⊘" };
export const LEASE_WORD: Record<LeaseState, string> = { leased: "leased", submitted: "update submitted", verified: "verified", rejected: "rejected" };
const LEASE_VAR: Record<LeaseState, string> = {
  leased: "var(--fl-lease-leased)",
  submitted: "var(--fl-lease-submitted)",
  verified: "var(--fl-lease-verified)",
  rejected: "var(--fl-lease-rejected)",
};

/** Your lease first, the rest in the order given. */
export function orderLeases(leases: Lease[]): Lease[] {
  return [...leases.filter((l) => l.mine), ...leases.filter((l) => !l.mine)];
}

/** The sentence for one lease: "Lease 7: rejected, the model update failed the file format check". */
export function leaseSentence(l: Lease): string {
  const who = l.mine ? `Your lease ${l.id}` : `Lease ${l.id}`;
  return l.state === "rejected" && l.reason ? `${who}: rejected, ${l.reason}` : `${who}: ${LEASE_WORD[l.state]}`;
}

/** Counts per state for a summary line. */
export function leaseCounts(leases: Lease[]): Record<LeaseState, number> {
  const out: Record<LeaseState, number> = { leased: 0, submitted: 0, verified: 0, rejected: 0 };
  for (const l of leases) out[l.state] += 1;
  return out;
}

export function LeaseDots({ leases, selected, onSelect }: { leases: Lease[]; selected?: string | null; onSelect?: (id: string) => void }) {
  const list = orderLeases(leases);
  const n = leaseCounts(leases);
  const summary = `${leases.length} leases: ${n.verified} verified, ${n.submitted} submitted, ${n.leased} leased, ${n.rejected} rejected`;
  return (
    <div data-testid="lease-dots" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <span className="sr-only">{summary}</span>
      <ul aria-label="Leases" style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexWrap: "wrap", gap: 6 }}>
        {list.map((l) => {
          const on = selected === l.id;
          return (
            <li key={l.id}>
              <button
                type="button"
                className="btn btn-sm fl-lease"
                data-lease={l.id}
                data-state={l.state}
                aria-pressed={onSelect ? on : undefined}
                aria-label={leaseSentence(l)}
                title={leaseSentence(l)}
                onClick={onSelect ? () => onSelect(l.id) : undefined}
                style={{ minWidth: 44, gap: 6, color: LEASE_VAR[l.state], borderColor: on ? "var(--tx-1)" : l.mine ? "var(--line-strong)" : "var(--line-2)", background: on ? "var(--srf-2)" : "transparent" }}
              >
                <span aria-hidden="true" style={{ fontSize: 14, lineHeight: 1 }}>{LEASE_GLYPH[l.state]}</span>
                <span className="mono" style={{ fontSize: 11, color: "var(--tx-2)" }}>{l.mine ? "you" : l.id}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
