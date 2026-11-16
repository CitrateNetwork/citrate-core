// =====================================================================
// citrate-core: a payout receipt (FL design board 12, "Payout receipt")
//
// Units × rate = amount, the tx hash, block and claim status, with the caller's actions. SALT
// only: the amount is what WorkMarket settled, always text, never a count-up. The status chip
// swaps on PayoutPosted, Settled or Disputed. Dark and not-deployed show no amount at all.
// Source: WorkMarket.settled(offer, worker) + its receipt.
// =====================================================================
import type { ReactNode } from "react";
import { saltFromWei } from "./format";
import { NOT_AVAILABLE, PAYOUTS_DARK } from "./networkState";
import { Chip, Hex, Unknown, type Tone } from "./primitives";

/** The board's statuses: WorkMarket.settle's blocking reasons (myWork.ts PayoutStatusWord), plus
 *  nothing, dark (G-SEC) and not deployed. */
export type PayoutStatus = "nothing" | "waiting" | "claimable" | "claimed" | "disputed" | "no-payout-list" | "challenge-open" | "reserve-swept" | "not-joined" | "dark" | "not-deployed";

export const PAYOUT_WORD: Record<PayoutStatus, string> = {
  nothing: "Nothing to claim",
  waiting: "Waiting for dispute window",
  claimable: "Claimable",
  claimed: "Claimed",
  disputed: "Disputed, frozen",
  "no-payout-list": "No payout list is live",
  "challenge-open": "A challenge to the payout list is open",
  "reserve-swept": "The unsettled reserve was returned to the poster",
  "not-joined": "You did not join this round",
  dark: PAYOUTS_DARK,
  "not-deployed": NOT_AVAILABLE,
};

const TONE: Record<PayoutStatus, Tone> = {
  nothing: "neutral",
  waiting: "info",
  claimable: "ok",
  claimed: "neutral",
  disputed: "warn",
  "no-payout-list": "neutral",
  "challenge-open": "warn",
  "reserve-swept": "neutral",
  "not-joined": "neutral",
  dark: "neutral",
  "not-deployed": "neutral",
};

export interface PayoutLine {
  round: string;
  /** null = not read: your units are a leaf of the coordinator's posted payout list. */
  units: bigint | null;
  rateWei: bigint;
  amountWei: bigint | null;
  txHash?: string;
  block?: number;
}

export function payoutSentence(status: PayoutStatus, line?: PayoutLine): string {
  if (!line || status === "dark" || status === "not-deployed" || status === "nothing") return PAYOUT_WORD[status];
  if (line.units === null || line.amountWei === null) return `Round ${line.round}: ${saltFromWei(line.rateWei)} SALT per unit, your units not read, ${PAYOUT_WORD[status].toLowerCase()}`;
  return `Round ${line.round}: ${line.units} units times ${saltFromWei(line.rateWei)} SALT equals ${saltFromWei(line.amountWei)} SALT, ${PAYOUT_WORD[status].toLowerCase()}`;
}

export function PayoutReceipt({ status, line, actions }: { status: PayoutStatus; line?: PayoutLine; actions?: ReactNode }) {
  const showAmount = !!line && status !== "dark" && status !== "not-deployed" && status !== "nothing";
  return (
    <article className="fl-card" data-testid="payout-receipt" data-status={status} aria-label={payoutSentence(status, line)} style={{ padding: "12px 14px", display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
        {line && <span style={{ fontSize: 13.5, fontWeight: 500 }}>Round {line.round}</span>}
        <Chip tone={TONE[status]}>{PAYOUT_WORD[status]}</Chip>
      </div>
      {showAmount && line && line.units !== null && line.amountWei !== null && (
        <span className="mono" style={{ fontSize: 13 }}>
          {line.units.toString()} units × {saltFromWei(line.rateWei)} SALT = {saltFromWei(line.amountWei)} SALT
        </span>
      )}
      {showAmount && line && (line.units === null || line.amountWei === null) && (
        <span style={{ fontSize: 13 }}>
          <span className="mono">{saltFromWei(line.rateWei)} SALT per unit</span> · your units: <Unknown reason="they are in the coordinator's payout list, which the app does not read yet" />
        </span>
      )}
      {showAmount && line?.txHash && (
        <span style={{ fontSize: 12 }} className="fl-muted">
          tx <Hex value={line.txHash} />
          {line.block !== undefined ? ` · block ${line.block.toLocaleString("en-US")}` : ""}
        </span>
      )}
      {actions && <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>{actions}</div>}
    </article>
  );
}
