// =====================================================================
// PolicyProposalCard: a worker-terms proposal, rendered by core in the transcript (FL design
// board 01, "Approvals in the thread, rendered by core").
//
// Hermes can only ask for this card; it cannot render or press its buttons. Its facts come from
// the proposal core validated (src/fl/workerTerms.ts termsProblem), and Confirm is the member's
// HIC-1 decision: the only caller of saveWorkerTerms from chat. Until core's worker-policy storage
// ships, saving is refused and the card says "Save failed, nothing changed". Notifications only
// point at this card; they never carry its buttons. No tool asks for this card yet: the Hermes
// worker_terms_propose tool ships with core's worker-policy storage (core #689).
// =====================================================================
import type { ReactNode } from "react";
import { Hex, HicBadge, KV, Unknown } from "../fl/primitives";
import { useAuthority } from "../fl/authority";
import { dataClassWords } from "../agent/workerProposal";
import type { WorkerTerms } from "../fl/workerTerms";
import type { ChatProposal } from "../shell/state";

/** Where "Edit fields" and "Change in Settings › Work" go: board 11, Settings › Work. No route ships
 *  on main yet (Lane 1 is building it). If board 11 lands under another name, change it here only. */
export const SETTINGS_WORK_ROUTE = "settings/work";

/** "Stored" says only what core's save receipt says; nothing is claimed before it. */
function storedLine(p: ChatProposal): ReactNode {
  if (p.state === "confirmed" && p.receipt) {
    return p.receipt.sealed ? (
      <>
        Sealed on this device · terms <Hex value={p.receipt.termsHash} />
      </>
    ) : (
      <>
        Saved on this device, not sealed · terms <Hex value={p.receipt.termsHash} />
      </>
    );
  }
  if (p.state === "confirmed") return <Unknown reason="no save receipt was returned" />;
  if (p.state === "failed" || p.state === "declined" || p.state === "expired") return "Nothing was saved";
  return "Not saved yet. Confirm saves it on this device";
}

function hoursLine(t: WorkerTerms): string {
  const when = t.hours === "any" ? "Any time" : `${t.hours.start} to ${t.hours.end}`;
  return t.weekdaysOnly ? `${when}, Monday to Friday` : when;
}

function at(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function PolicyProposalCard({
  proposal,
  onConfirm,
  onDecline,
  onReopen,
  onEdit,
}: {
  proposal: ChatProposal;
  onConfirm: () => void;
  onDecline: () => void;
  onReopen: () => void;
  onEdit: () => void;
}) {
  const t = proposal.terms;
  // Auto-join is the authority in force: HIC-1 asks each time; an envelope the member signed joins inside its caps.
  const authority = useAuthority();
  const hid = "proposal-h-" + proposal.id;
  return (
    <section
      data-testid="policy-proposal"
      data-state={proposal.state}
      aria-labelledby={hid}
      tabIndex={-1}
      style={{ border: "1px solid var(--line-2)", borderRadius: 6, background: "var(--srf-2)" }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 14px", background: "var(--srf-inset, var(--srf-1))", borderBottom: "1px solid var(--line-1)", flexWrap: "wrap" }}>
        <HicBadge level="HIC-1" />
        <h3 id={hid} style={{ margin: 0, fontSize: 14, fontWeight: 600 }}>
          Hermes proposes a worker policy
        </h3>
        <span style={{ flex: 1 }}></span>
        <span style={{ fontSize: 11, color: "var(--tx-3)" }}>Rendered by Citrate Core. Hermes cannot press these buttons.</span>
      </div>
      <KV
        style={{ padding: "12px 14px", margin: 0, fontSize: 12.5 }}
        rows={[
          { k: "Floor", v: `${t.floorSaltPerUnit} SALT per unit`, mono: true },
          { k: "Hours", v: hoursLine(t) },
          { k: "Power", v: t.mainsOnly ? "Mains only; pause on battery" : "Mains or battery" },
          { k: "Data rounds may read", v: t.dataClasses.length === 0 ? "None. A round reads only datasets you add to it, on this device." : t.dataClasses.map(dataClassWords).join(", ") + ", on this device only" },
          { k: "Auto-join", v: authority.level === "HIC-2" ? `Inside your envelope "${authority.name}" (HIC-2)` : "Off. Each join asks you (HIC-1)." },
          { k: "Stored", v: storedLine(proposal) },
        ]}
      />
      <div role="status" aria-live="polite" style={{ display: "flex", gap: 8, padding: "10px 14px", borderTop: "1px solid var(--line-1)", flexWrap: "wrap", alignItems: "center" }}>
        {proposal.state === "pending" && (
          <>
            <button type="button" className="btn btn-primary" data-testid="proposal-confirm" onClick={onConfirm}>
              Confirm policy
            </button>
            <button type="button" className="btn" data-testid="proposal-edit" onClick={onEdit}>
              Edit fields
            </button>
            <button type="button" className="btn btn-ghost" data-testid="proposal-decline" onClick={onDecline}>
              Not now
            </button>
          </>
        )}
        {proposal.state === "saving" && <span style={{ fontSize: 12.5 }}>Saving on this device</span>}
        {proposal.state === "confirmed" && (
          <>
            <span className="fl-chip fl-chip-ok">Confirmed by you{proposal.at ? ` at ${at(proposal.at)}` : ""}</span>
            <button type="button" className="btn btn-ghost btn-sm" onClick={onEdit}>
              Change in Settings › Work
            </button>
          </>
        )}
        {proposal.state === "declined" && (
          <>
            <span className="fl-chip">Not now · nothing changed</span>
            <button type="button" className="btn btn-sm" data-testid="proposal-reopen" onClick={onReopen}>
              Review again
            </button>
          </>
        )}
        {proposal.state === "expired" && (
          <span className="fl-chip" data-testid="proposal-expired">
            From an earlier session. It can no longer be confirmed; ask Hermes to propose again.
          </span>
        )}
        {proposal.state === "failed" && (
          <>
            <span className="fl-chip fl-chip-danger" data-testid="proposal-failed">
              Save failed, nothing changed
            </span>
            <span style={{ fontSize: 12, color: "var(--tx-2)" }}>{proposal.error}</span>
            <button type="button" className="btn btn-sm" data-testid="proposal-reopen" onClick={onReopen}>
              Review again
            </button>
          </>
        )}
      </div>
    </section>
  );
}
