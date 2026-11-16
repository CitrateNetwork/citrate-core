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
import { Chip, Hex, type Tone } from "./primitives";

export type PayoutStatus = "nothing" | "waiting" | "claimable" | "claimed" | "disputed" | "dark" | "not-deployed";

export const PAYOUT_WORD: Record<PayoutStatus, string> = {
  nothing: "Nothing to claim",
  waiting: "Waiting for dispute window",
  claimable: "Claimable",
  claimed: "Claimed",
  disputed: "Disputed, frozen",
  dark: PAYOUTS_DARK,
  "not-deployed": NOT_AVAILABLE,
};

const TONE: Record<PayoutStatus, Tone> = {
  nothing: "neutral",
  waiting: "info",
  claimable: "ok",
  claimed: "neutral",
  disputed: "warn",
  dark: "neutral",
  "not-deployed": "neutral",
};

export interface PayoutLine {
  round: string;
  units: bigint;
  rateWei: bigint;
  amountWei: bigint;
  txHash?: string;
  block?: number;
}

export function payoutSentence(status: PayoutStatus, line?: PayoutLine): string {
  if (!line || status === "dark" || status === "not-deployed" || status === "nothing") return PAYOUT_WORD[status];
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
      {showAmount && line && (
        <span className="mono" style={{ fontSize: 13 }}>
          {line.units.toString()} units × {saltFromWei(line.rateWei)} SALT = {saltFromWei(line.amountWei)} SALT
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
