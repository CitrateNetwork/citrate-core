// =====================================================================
// citrate-core: Groups › Rounds › Live round (FL design board 06)
//
// The ring with lease dots, the challenge clock, Belnap counts, escrow, your contribution, and
// the round's facts, in all eight states: pending, running, verifying, accepted, rejected,
// disputed, expired, and coordinator not answering. Sources, per part: WorkMarket events and
// getOffer (ring, escrow), the coordinator's /v1/status (leases, labelled coordinator-reported),
// FederatedRoundLedger's window end and eth_blockNumber (clock), the FRL commit's Belnap
// aggregate, and the local worker log plus coordinator receipt (your contribution).
//
// Ship default: no WorkMarket on r1007, so the route shows the honest Today state. With WorkMarket
// deployed but no per-lease read model in this build, it says there is no round to show. The view
// below takes a LiveRound and is tested with fixtures only.
// =====================================================================
import { useState, type ReactNode } from "react";
import { BelnapCounts, type Belnap } from "./BelnapCounts";
import { ChallengeClock, clockSentence, type ChallengeClockProps } from "./ChallengeClock";
import { ContributionTimeline, type ContributionStep } from "./ContributionTimeline";
import { EscrowBar, type Escrow } from "./EscrowBar";
import { saltFromWei } from "./format";
import { LeaseDots, leaseCounts, leaseSentence, orderLeases, type Lease } from "./LeaseDots";
import { NOT_AVAILABLE, PAYOUTS_DARK, type PayoutsState } from "./networkState";
import { Card, Chip, Eyebrow, Hex, KV, Note, Unknown, type Tone } from "./primitives";
import { RoundRing, type RingOutcome, type RingStage } from "./RoundRing";

export type LiveState = "pending" | "running" | "verifying" | "accepted" | "rejected" | "disputed" | "expired";

export interface LiveRound {
  offerId: number;
  /** The poster as the roster names them, or their address. */
  poster: string;
  state: LiveState;
  /** False when the coordinator's last /v1/status poll failed. */
  coordinatorAnswering: boolean;
  /** Seconds since the coordinator last answered, when it is not answering. */
  coordinatorLastSeenSecs?: number;
  /** null = the coordinator has not reported leases. */
  leases: Lease[] | null;
  maxWorkers: number;
  clock: ChallengeClockProps;
  belnap: Belnap | null;
  belnapReason?: string;
  escrow: Escrow | null;
  escrowReason?: string;
  contribution: ContributionStep[];
  /** The FRL fault word for a rejected round ("output mismatch"). */
  fault?: string;
  /** The block FRL accepted the round at. */
  acceptedBlock?: number;
  rateWei: bigint;
  deadline: number;
  payouts: PayoutsState;
  /** Your settled line, when a payout list is posted. */
  payout?: { units: bigint; amountWei: bigint } | null;
  ids: { frlRound: string | null; mergedHash: string | null; coordinator: string | null };
}

const RING: Record<LiveState, { stage: RingStage; outcome?: RingOutcome }> = {
  pending: { stage: "open" },
  running: { stage: "running" },
  verifying: { stage: "verifying" },
  accepted: { stage: "payout" },
  rejected: { stage: "verifying", outcome: "rejected" },
  disputed: { stage: "payout", outcome: "disputed" },
  expired: { stage: "open", outcome: "expired" },
};

const WORD: Record<LiveState, { word: string; tone: Tone }> = {
  pending: { word: "Pending", tone: "info" },
  running: { word: "Running", tone: "ok" },
  verifying: { word: "Verifying", tone: "ok" },
  accepted: { word: "Accepted", tone: "ok" },
  rejected: { word: "Rejected", tone: "danger" },
  disputed: { word: "Disputed", tone: "warn" },
  expired: { word: "Expired", tone: "neutral" },
};

export const NEXT: Record<LiveState, string> = {
  pending: "Workers take leases until every place is out or the deadline passes. Training starts on each machine inside its hours.",
  running: "The coordinator checks each update as it arrives, then merges them into one adapter and commits it on chain.",
  verifying: "Anyone can challenge the merge until the window ends. If nobody proves it wrong, the round is Accepted.",
  accepted: "The payout list is posted. Workers can dispute until the dispute window ends. Then each worker claims.",
  rejected: "No payout. The poster can reclaim the escrow. The merged adapter cannot pass the adapter checks.",
  disputed: "Payouts stay frozen until the dispute resolves. If the worker's signed receipt holds, the coordinator bond can be slashed.",
  expired: "The full budget is refundable to the poster. Nobody is paid.",
};

/** The state line beside the title, in truth contract words ("Accepted on chain at block N"). */
export function stateLine(r: LiveRound): string {
  const n = r.leases ? leaseCounts(r.leases) : null;
  switch (r.state) {
    case "pending":
      return n ? `Waiting for leases · ${r.leases!.length} of ${r.maxWorkers} taken` : "Waiting for leases";
    case "running":
      return n ? `${r.leases!.length} leases out · ${n.submitted + n.verified} submitted` : "Leases out";
    case "verifying":
      return r.clock.commitBlock !== undefined ? `Merge committed at block ${r.clock.commitBlock.toLocaleString("en-US")} · window open` : "Merge committed · window open";
    case "accepted":
      return r.acceptedBlock !== undefined ? `Accepted on chain at block ${r.acceptedBlock.toLocaleString("en-US")} · payout list posted` : "Accepted on chain · payout list posted";
    case "rejected":
      return `Challenge proved${r.fault ? " · " + r.fault : ""}`;
    case "disputed":
      return "A worker disputed the payout list · payout frozen";
    case "expired":
      return "Deadline passed before enough updates · full refund, no payout";
  }
}

/** The ring's text equivalent. */
export function ringLabel(r: LiveRound): string {
  const n = r.leases ? leaseCounts(r.leases) : null;
  const leases = n ? `${n.verified} of ${r.maxWorkers} leases verified` : "leases not reported";
  return `Round ${r.offerId}: ${WORD[r.state].word.toLowerCase()}, ${leases}, ${clockSentence(r.clock).toLowerCase()}`;
}

export function LiveRoundView({ r, go }: { r: LiveRound; go: (route: string) => void }) {
  const [sel, setSel] = useState<string | null>(null);
  const [ids, setIds] = useState(false);
  const ring = RING[r.state];
  const w = WORD[r.state];
  const silent = !r.coordinatorAnswering;
  const picked = sel && r.leases ? r.leases.find((l) => l.id === sel) : null;
  const n = r.leases ? leaseCounts(r.leases) : null;
  const payoutLine = (): ReactNode => {
    if (r.payouts === "not-deployed") return NOT_AVAILABLE;
    if (r.payouts === "dark") return PAYOUTS_DARK;
    if (r.payouts === "unknown") return <Unknown reason="the payout state was not read" />;
    if (r.state === "disputed") return "Frozen while the dispute is open.";
    if (r.state === "accepted" && r.payout) return `${r.payout.units} units × ${saltFromWei(r.rateWei)} SALT = ${saltFromWei(r.payout.amountWei)} SALT. Claim opens after the dispute window.`;
    return "Nothing to claim yet.";
  };
  return (
    <div className="fl-cols" data-testid="live-round" data-state={r.state} data-silent={silent}>
      <section className="fl-main" aria-labelledby="live-h">
        <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
          <h1 id="live-h" className="fl-disp" style={{ fontSize: 22 }}>
            Round {r.offerId} · {r.poster}
          </h1>
          <Chip tone={silent ? "warn" : w.tone}>{w.word}</Chip>
          <span className="fl-muted" style={{ fontSize: 12.5 }}>
            {silent ? "Coordinator not answering" : stateLine(r)}
          </span>
        </div>
        {silent && (
          <div role="status" className="fl-card" style={{ padding: "8px 12px", borderColor: "var(--warn)", background: "var(--warn-bg)", color: "var(--warn)", fontSize: 12.5 }}>
            Coordinator not answering{r.coordinatorLastSeenSecs !== undefined ? ` (last seen ${Math.max(1, Math.round(r.coordinatorLastSeenSecs / 60))} min ago)` : ""}. Lease counts are the last coordinator report. On-chain state is unaffected.
          </div>
        )}
        {r.state === "rejected" && (
          <div role="status" className="fl-card" style={{ padding: "8px 12px", borderColor: "var(--danger)", background: "var(--danger-bg)", color: "var(--danger)", fontSize: 12.5 }}>
            Rejected{r.fault ? `: ${r.fault}` : ""}. A challenger replayed the merge and it did not match. A rejected round never pays; the escrow is refundable to the poster.
          </div>
        )}
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(320px, 1fr))", gap: 12 }}>
          <Card label="Round ring and leases" as="section" style={{ flexDirection: "row", gap: 16, alignItems: "center", flexWrap: "wrap" }}>
            <RoundRing stage={ring.stage} outcome={ring.outcome} label={ringLabel(r)} size={180} legend />
            <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0, flex: 1 }}>
              <Eyebrow>Leases · select one</Eyebrow>
              {r.leases ? (
                <>
                  <LeaseDots leases={orderLeases(r.leases)} selected={sel} onSelect={setSel} />
                  <span className="fl-muted" style={{ fontSize: 11.5 }} aria-live="polite">
                    {picked ? leaseSentence(picked) : `${n!.verified} verified · ${n!.submitted} submitted · ${n!.leased} leased · ${n!.rejected} rejected`}
                  </span>
                </>
              ) : (
                <span style={{ fontSize: 12.5 }}>
                  <Unknown reason="the coordinator has not reported leases" />
                </span>
              )}
              <span className="fl-faint" style={{ fontSize: 11 }}>coordinator-reported</span>
            </div>
          </Card>
          <div style={{ display: "flex", flexDirection: "column", gap: 12, minWidth: 0 }}>
            <Card>
              <Eyebrow>Challenge window</Eyebrow>
              <ChallengeClock {...r.clock} />
            </Card>
            <Card>
              <Eyebrow>Belnap result</Eyebrow>
              <BelnapCounts counts={r.belnap} reason={r.belnapReason ?? "no merge committed yet"} />
            </Card>
            <Card>
              <Eyebrow>Escrow</Eyebrow>
              <EscrowBar escrow={r.escrow} reason={r.escrowReason} />
            </Card>
          </div>
        </div>
        <Card label="Your contribution" as="section">
          <Eyebrow>Your contribution</Eyebrow>
          <ContributionTimeline steps={r.contribution} />
        </Card>
      </section>
      <aside className="fl-rail" aria-label="Round facts">
        <Card>
          <Eyebrow>What happens next</Eyebrow>
          <span style={{ fontSize: 12.5, lineHeight: 1.5 }}>{silent ? "Your update stays on this device. Nothing is lost if the coordinator returns." : NEXT[r.state]}</span>
        </Card>
        <Card>
          <KV
            rows={[
              { k: "Poster", v: r.poster },
              { k: "Rate", v: `${saltFromWei(r.rateWei)} SALT per unit`, mono: true },
              { k: "Workers", v: `${r.maxWorkers} max` },
              { k: "Deadline", v: new Date(r.deadline * 1000).toISOString().slice(0, 10), mono: true },
            ]}
          />
        </Card>
        <Card testid="live-payout">
          <Eyebrow>Payout</Eyebrow>
          <span style={{ fontSize: 12.5 }}>{payoutLine()}</span>
          <button type="button" className="btn btn-sm btn-ghost" style={{ alignSelf: "flex-start" }} onClick={() => go("groups/rounds/payouts")}>
            Receipts and claims
          </button>
          {r.state === "accepted" && r.payouts === "open" && (
            <button type="button" className="btn btn-sm btn-danger" style={{ alignSelf: "flex-start" }} onClick={() => go("groups/rounds/payouts")}>
              My lease is missing: dispute
            </button>
          )}
        </Card>
        {r.state === "accepted" && (
          <button type="button" className="btn btn-primary" onClick={() => go("groups/rounds/adapters")}>
            Check the merged adapter
          </button>
        )}
        <button type="button" className="btn btn-sm btn-ghost" aria-expanded={ids} onClick={() => setIds(!ids)}>
          {ids ? "Hide ids and hashes" : "Show ids and hashes"}
        </button>
        {ids && (
          <KV
            style={{ fontSize: 11.5 }}
            rows={[
              { k: "Offer id", v: String(r.offerId), mono: true },
              { k: "FRL round", v: r.ids.frlRound ? <Hex value={r.ids.frlRound} /> : <Unknown reason="not committed yet" /> },
              { k: "Merged hash", v: r.ids.mergedHash ? <Hex value={r.ids.mergedHash} /> : <Unknown reason="not committed yet" /> },
              { k: "Coordinator", v: r.ids.coordinator ? <span className="mono">{r.ids.coordinator}</span> : <Unknown reason="not configured" /> },
            ]}
          />
        )}
      </aside>
    </div>
  );
}

/** WorkMarket is deployed but this build cannot read your round (no per-lease read model yet). */
export function NoLiveRound({ go }: { go: (route: string) => void }) {
  return (
    <section className="fl-main" data-testid="live-none">
      <h1 className="fl-disp" style={{ fontSize: 22 }}>
        No live round to show
      </h1>
      <p className="fl-muted" style={{ margin: 0, fontSize: 13 }}>
        You are not in a round on this device. A round you join appears here.
      </p>
      <Note>This build reads offers, not the leases inside a round, so a round's ring, leases and receipts appear once that read model ships.</Note>
      <button type="button" className="btn btn-ghost" style={{ alignSelf: "flex-start" }} onClick={() => go("groups/rounds")}>
        See matches
      </button>
    </section>
  );
}
