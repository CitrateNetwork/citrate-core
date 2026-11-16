// =====================================================================
// RoundDraftCard: a round draft Hermes proposed, rendered by core in the transcript (FL design
// boards 01 and 07). Hermes cannot press its buttons. "Review in Open a round" is the member's
// click: it hands the draft to Groups › Rounds › Open a round (src/fl/roundDraft.ts setRoundDraft,
// marked as Hermes's), where the member checks every field and signs it themselves. Nothing is
// escrowed, published or opened from here.
// =====================================================================
import { HicBadge, KV } from "../fl/primitives";
import { BLANK_DRAFT, draftProblems, leasesCovered, type RoundDraft } from "../fl/roundDraft";
import type { ChatRoundDraft } from "../shell/state";

const TEMPLATE_WORDS: Record<RoundDraft["template"], string> = { tune: "Tune a model", eval: "Evaluate an adapter", data: "Build a dataset" };
const WHO_WORDS: Record<RoundDraft["who"], string> = { members: "Members of the group", group: "The whole group", anyone: "Anyone on the network" };

export function RoundDraftCard({ card, onOpen, onDecline }: { card: ChatRoundDraft; onOpen: () => void; onDecline: () => void }) {
  const d = card.draft;
  const full: RoundDraft = { ...BLANK_DRAFT, ...d, source: "hermes" };
  const problems = draftProblems(full);
  const leases = d.budgetSalt && d.rateSalt ? leasesCovered(d.budgetSalt, d.rateSalt) : null;
  const notSet = "not set; you fill it in Open a round";
  const hid = "round-draft-h-" + card.id;
  return (
    <section data-testid="round-draft" data-state={card.state} aria-labelledby={hid} style={{ border: "1px solid var(--line-2)", borderRadius: 6, background: "var(--srf-2)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 14px", background: "var(--srf-inset, var(--srf-1))", borderBottom: "1px solid var(--line-1)", flexWrap: "wrap" }}>
        <HicBadge level="HIC-1" />
        <h3 id={hid} style={{ margin: 0, fontSize: 14, fontWeight: 600 }}>
          Hermes drafted a round
        </h3>
        <span style={{ flex: 1 }}></span>
        <span style={{ fontSize: 11, color: "var(--tx-3)" }}>Rendered by Citrate Core. Hermes cannot press these buttons.</span>
      </div>
      <KV
        style={{ padding: "12px 14px", margin: 0, fontSize: 12.5 }}
        rows={[
          { k: "Template", v: d.template ? TEMPLATE_WORDS[d.template] : notSet },
          { k: "Data class", v: d.dataClass || notSet },
          { k: "Budget", v: d.budgetSalt ? `${d.budgetSalt} SALT` : notSet, mono: !!d.budgetSalt },
          { k: "Rate", v: d.rateSalt ? `${d.rateSalt} SALT per unit` : notSet, mono: !!d.rateSalt },
          { k: "Covers", v: leases === null ? "set a budget and a rate to see how many leases" : `${leases} ${leases === 1 ? "lease" : "leases"}` },
          { k: "Workers", v: d.maxWorkers !== undefined ? `up to ${d.maxWorkers}` : notSet },
          { k: "Deadline", v: d.deadline || notSet },
          { k: "Who may join", v: d.who ? WHO_WORDS[d.who] : notSet },
        ]}
      />
      {problems.length > 0 && card.state === "pending" && (
        <ul data-testid="round-draft-problems" style={{ margin: 0, padding: "0 14px 10px 32px", fontSize: 12, color: "var(--tx-2)" }}>
          {problems.map((p) => (
            <li key={p}>{p}</li>
          ))}
        </ul>
      )}
      <div role="status" aria-live="polite" style={{ display: "flex", gap: 8, padding: "10px 14px", borderTop: "1px solid var(--line-1)", flexWrap: "wrap", alignItems: "center" }}>
        {card.state === "pending" && (
          <>
            <button type="button" className="btn btn-primary" data-testid="round-draft-open" onClick={onOpen}>
              Review in Open a round
            </button>
            <button type="button" className="btn btn-ghost" data-testid="round-draft-decline" onClick={onDecline}>
              Not now
            </button>
            <span style={{ fontSize: 11.5, color: "var(--tx-3)" }}>You check every field and sign it there. Nothing is escrowed from here.</span>
          </>
        )}
        {card.state === "opened" && <span className="fl-chip fl-chip-ok">Opened in Open a round by you</span>}
        {card.state === "declined" && <span className="fl-chip">Not now · nothing was saved</span>}
        {card.state === "expired" && (
          <span className="fl-chip" data-testid="round-draft-expired">
            From an earlier session. Ask Hermes to draft it again.
          </span>
        )}
      </div>
    </section>
  );
}
