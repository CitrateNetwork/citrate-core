// =====================================================================
// citrate-core — round cards in a group's chat (0.5.4 FL design, board 02).
//
// Two cards, both rendered by core (Hermes output never renders buttons):
//   RoundDraftCard  a round Hermes drafted in the chat, with what each role may do with it.
//   RoundEventCard  a live round's state, updated from round events (0.6.0: WorkMarket + FRL).
// Neither offers joining a round from chat. Joining is in Groups › Rounds, and only when rounds
// are live.
//
// Today (0.5.4) the round ledgers are deployed and hold no rounds, and WorkMarket is not deployed,
// so the reading is "Group rounds not live yet" and no live round card can exist: `liveRoundsFor`
// has no source and returns none. The live card is built for the 0.6.0 events and tested with
// fixtures only. This is a clean seam: Lane 1's FL primitives replace `useRoundsToday` and
// `liveRoundsFor` when its foundation lands.
//
// Data sources (Rule 7): Lane 1's FL network state (src/fl/networkState.ts: the address book
// 40204.json, eth_getCode, and PatronageLedger.totalUnits()), shared with Groups › Rounds. The
// required words are its ROUNDS_NOT_LIVE and DEPLOYED_NO_ROUNDS (#676 is fixed there). The cards draw
// with the shared FL primitives (src/fl/primitives.tsx).
// =====================================================================
import { Card, Chip, HicBadge, KV, Unknown } from "../../fl/primitives";
import { DEPLOYED_NO_ROUNDS, ROUNDS_NOT_LIVE, type FlNetworkState } from "../../fl/networkState";
import { useFlNetworkState } from "../../fl/useFlNetworkState";
import type { GroupRole } from "../../bridge/domains";
import type { RoundDraft } from "./groupEnvelope";

export { ROUNDS_NOT_LIVE };

export type RoundsToday =
  | { kind: "reading" }
  /** No round can be offered on this network. `detail` says what the ledgers read. */
  | { kind: "not-live"; detail: string }
  /** Rounds can be offered (0.6.0: WorkMarket and FederatedRoundLedger deployed). */
  | { kind: "live" }
  /** A read the state depends on failed: unknown, shown as "—" with the reason. */
  | { kind: "unknown"; reason: string };

/** The FL network state in the chat's words. */
export function roundsToday(n: FlNetworkState): RoundsToday {
  switch (n.phase) {
    case "reading":
      return { kind: "reading" };
    case "unknown":
      return { kind: "unknown", reason: n.phaseReason ?? "a ledger read failed" };
    case "rounds-live":
      return { kind: "live" };
    case "today": {
      const frl = n.contracts.FederatedRoundLedger;
      const pl = n.contracts.PatronageLedger;
      const none = (c: typeof frl) => c.kind === "deployed" && c.rounds === "none";
      if (none(frl) && none(pl)) return { kind: "not-live", detail: `Both ledgers are ${DEPLOYED_NO_ROUNDS}.` };
      return { kind: "not-live", detail: "WorkMarket is not available yet on this network." };
    }
  }
}

/** One line for Hermes's context and the rail. */
export function roundsLine(t: RoundsToday): string {
  if (t.kind === "reading") return "being read";
  if (t.kind === "unknown") return `— (the ledger read failed: ${t.reason})`;
  if (t.kind === "live") return "rounds can be offered on this network";
  return `${ROUNDS_NOT_LIVE}. ${t.detail}`;
}

/** The round ledgers' state for the chat (one read shared with every FL view). */
export function useRoundsToday(): RoundsToday {
  return roundsToday(useFlNetworkState().state);
}

/** A live round's state, from round events (0.6.0). */
export interface LiveRoundSummary {
  round: number;
  stage: "Open" | "Running" | "Verifying" | "Payout" | "Closed";
  leases: number;
  verified: number;
  /** "in about 3 h 10 min", from the FRL window end and the head block. */
  windowEnds: string | null;
  /** Your lease's state in plain words, or null when you hold none. */
  yourLease: string | null;
}

/** The live rounds for a group. No source exists on this network (WorkMarket is not deployed). */
export function liveRoundsFor(_groupId: string): LiveRoundSummary[] {
  return [];
}

/** The card's full sentence (also its accessible name). */
export function liveRoundSentence(r: LiveRoundSummary): string {
  const parts = [`Round ${r.round}: ${r.stage.toLowerCase()}`, `${r.verified} of ${r.leases} leases verified`];
  if (r.windowEnds) parts.push(`challenge window ends ${r.windowEnds}`);
  if (r.yourLease) parts.push(`your lease ${r.yourLease}`);
  return parts.join(", ");
}

export function RoundEventCard({ r, onOpen }: { r: LiveRoundSummary; onOpen: () => void }) {
  const sentence = liveRoundSentence(r);
  return (
    <button type="button" className="fl-card" data-testid="round-event-card" onClick={onOpen} aria-label={`${sentence}. Open the live round.`} style={{ textAlign: "left", cursor: "pointer", color: "var(--tx-1)", padding: "12px 14px", display: "flex", flexDirection: "column", gap: 4 }}>
      <span style={{ fontWeight: 500 }}>Round {r.round} · {r.stage}</span>
      <span className="grp-muted" style={{ fontSize: 12 }}>
        {r.verified} of {r.leases} leases verified{r.windowEnds ? ` · window ends ${r.windowEnds}` : ""}{r.yourLease ? ` · your lease ${r.yourLease}` : ""}
      </span>
      <span className="fl-src" title="Read from WorkMarket and FederatedRoundLedger events">round events</span>
    </button>
  );
}

const salt = (n: number) => `${n.toLocaleString("en-US", { maximumFractionDigits: 4 })} SALT`;

/** What a role sees on a draft. */
export function draftActions(role: GroupRole | null): "review" | "wait" | "read-only" {
  if (role === "owner" || role === "admin") return "review";
  if (role === "guest") return "read-only";
  return "wait";
}

/**
 * A round draft from a Hermes answer. `verified` is true only for an answer this device's Hermes
 * posted; a draft in any other message is shown as what it is, a member's message, and never gets
 * the Review action.
 */
export function RoundDraftCard({ draft, rounds, myRole, onReview, verified }: { draft: RoundDraft; rounds: RoundsToday; myRole: GroupRole | null; onReview: () => void; verified: boolean }) {
  const act = verified ? draftActions(myRole) : "unverified";
  const leases = draft.budgetSalt !== null && draft.rateSalt ? Math.floor(draft.budgetSalt / draft.rateSalt) : null;
  const hId = "draft-" + draft.title.replace(/[^a-z0-9]+/gi, "-").toLowerCase().slice(0, 40);
  const notProposed = <Unknown reason="not proposed" />;
  return (
    <Card as="section" raised labelledBy={hId} testid="round-draft-card">
      <span data-rounds={rounds.kind} data-verified={verified ? "yes" : "no"} style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <HicBadge level="HIC-1" />
        <h3 id={hId} className="grp-h2">Round draft: {draft.title}</h3>
        <Chip>Draft · no SALT moved</Chip>
      </span>
      <KV
        rows={[
          { k: "Budget", v: draft.budgetSalt !== null ? <><span className="mono">{salt(draft.budgetSalt)}</span>, held in escrow only when an owner or admin opens it</> : notProposed },
          { k: "Rate", v: draft.rateSalt !== null ? <><span className="mono">{salt(draft.rateSalt)}</span> per lease · comparable rounds: none on this network yet</> : notProposed },
          { k: "Workers", v: draft.workers !== null ? `Up to ${draft.workers}${leases !== null ? ` · budget covers ${leases.toLocaleString("en-US")} leases` : ""}` : notProposed },
          { k: "Deadline", v: draft.deadline ?? notProposed },
        ]}
      />
      <span className="grp-note">
        {verified
          ? "Hermes proposed these values. Rendered by Citrate Core. Hermes cannot press these buttons."
          : "Not verified as Hermes output: these values came in a member's message. To use them, open a round yourself in Groups › Rounds."}
      </span>
      <span style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        {act === "review" && (
          <button className="btn btn-sm" onClick={onReview}>{rounds.kind === "live" ? "Review and sign to open" : "Review draft"}</button>
        )}
        {act === "wait" && <span className="grp-muted" style={{ fontSize: 12 }}>Waiting for an owner or admin to review it.</span>}
        {act === "read-only" && <span className="grp-muted" style={{ fontSize: 12 }}>Read-only members can see drafts but not act on them.</span>}
        {rounds.kind === "not-live" && <Chip tone="warn">{ROUNDS_NOT_LIVE}</Chip>}
        {rounds.kind === "unknown" && <Chip title={`The round ledger read failed: ${rounds.reason}`}>Rounds: —</Chip>}
      </span>
    </Card>
  );
}
