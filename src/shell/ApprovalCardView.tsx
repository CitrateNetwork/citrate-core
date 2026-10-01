// HUP-S2.4 — renders an approval card (src/agent/approvalCards.ts) inside the existing approval UI:
// the SignatureCeremony and the wallet-review modal. Pure view; the decision buttons stay in those
// modals, so a card adds facts and never a way to resolve.
import type { ApprovalCard, HicRequirement } from "../agent/approvalCards";

const label = { fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase" as const, color: "var(--tx-3)" };

/** The banner for a hic:"required" call: why, and that only the member's click decides it. */
export function HicBanner({ hic }: { hic: HicRequirement }) {
  return (
    <div data-testid="hic-banner" role="alert" style={{ border: "1px solid var(--warn)", background: "var(--warn-bg)", borderRadius: "var(--r-1)", padding: "10px 14px", display: "flex", flexDirection: "column", gap: 4 }}>
      <span className="mono" style={{ ...label, color: "var(--warn)" }}>
        Your explicit decision
      </span>
      <span style={{ fontSize: 12, lineHeight: 1.5, color: "var(--warn)" }}>{hic.reason}</span>
      <span style={{ fontSize: 11.5, lineHeight: 1.5, color: "var(--tx-2)" }}>Nothing runs unless you press Approve. There is no automatic approval for this request.</span>
    </div>
  );
}

/** One card: the summary line, then the facts in the card's own form. */
export function ApprovalCardView({ card, showRows = true }: { card: ApprovalCard; showRows?: boolean }) {
  return (
    <div data-testid={"card-" + card.kind} style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div data-testid="card-summary" style={{ fontSize: 13, lineHeight: 1.45, color: "var(--tx-1)" }}>
        {card.summary}
      </div>
      {card.kind === "diff" && (
        <div style={{ border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", overflow: "hidden" }}>
          <div className="mono" style={{ ...label, padding: "6px 12px", borderBottom: "1px solid var(--line-1)", background: "var(--srf-1)" }}>
            {card.path}
            {card.created ? " · new file" : ""}
          </div>
          {/* HUP-S10.6: a scrollable region must be reachable and named for keyboard members. */}
          <pre className="mono" tabIndex={0} role="region" aria-label={"Changes to " + card.path} style={{ margin: 0, padding: "6px 0", maxHeight: 220, overflow: "auto", fontSize: 11.5, lineHeight: 1.5 }}>
            {card.lines.map((l, i) => (
              <div
                key={i}
                data-op={l.op}
                style={{
                  padding: "0 12px",
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-all",
                  background: l.op === "add" ? "var(--ok-bg)" : l.op === "remove" ? "var(--warn-bg)" : "transparent",
                  color: l.op === "context" ? "var(--tx-3)" : "var(--tx-1)",
                }}
              >
                {(l.op === "add" ? "+ " : l.op === "remove" ? "- " : "  ") + l.text}
              </div>
            ))}
          </pre>
        </div>
      )}
      {card.kind === "command" && (
        <div style={{ border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: "8px 12px", background: "var(--srf-1)", display: "flex", flexDirection: "column", gap: 4 }}>
          <span className="mono" style={label}>
            Exact arguments{card.cwd ? " · in " + card.cwd : ""}
          </span>
          <ol className="mono" style={{ margin: 0, paddingLeft: 22, fontSize: 12 }}>
            {card.argv.map((a, i) => (
              <li key={i} data-testid="argv" style={{ wordBreak: "break-all" }}>
                {JSON.stringify(a)}
              </li>
            ))}
          </ol>
        </div>
      )}
      {(card.kind === "fields" || card.kind === "chain") && showRows && card.rows.length > 0 && (
        <div style={{ border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", overflow: "hidden" }}>
          {card.rows.map((r, i) => (
            <div key={i} style={{ display: "flex", gap: 14, padding: "7px 12px", borderBottom: i < card.rows.length - 1 ? "1px solid var(--line-1)" : "none", background: "var(--srf-1)" }}>
              <span className="mono" style={{ ...label, width: 92, flexShrink: 0, paddingTop: 2 }}>
                {r.k}
              </span>
              <span className="mono" style={{ fontSize: 12, color: "var(--tx-1)", wordBreak: "break-all" }}>
                {r.v}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
