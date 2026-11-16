// =====================================================================
// citrate-core: the ceremony sheet (FL design board 12, "Ceremony sheet")
//
// Review, signing (three named steps), then the outcome in UI truth contract words. Built on
// ModalDialog: focus starts on the safe choice (Decline), Tab stays inside, and Escape declines
// only before signing. Progress is a polite status region. The sheet never signs anything: the
// caller's `onSign` hands the reviewed intent to the SignatureCeremony (store.requestSig or a
// pending ceremony view), and the sheet only reports what came back.
// =====================================================================
import type { ReactNode } from "react";
import { ModalDialog } from "../shell/ModalDialog";
import { broadcastFailure } from "../shell/broadcastOutcome";
import { saltFromWei } from "./format";
import { HicBadge, shortHex, type HicLevel } from "./primitives";

export type TxOutcome =
  | { kind: "included"; block: number }
  | { kind: "reverted"; block: number; feeWei: bigint | null; reason: string | null }
  | { kind: "not-sent"; reason: string }
  | { kind: "unknown"; txHash: string | null; reason?: string };

export interface OutcomeWords {
  tone: "ok" | "warn" | "danger";
  word: string;
  line: string;
}

/** Truth contract §1 words for a transaction's outcome. Never "final" or "settled". */
export function outcomeWords(o: TxOutcome, success = "The network included it."): OutcomeWords {
  switch (o.kind) {
    case "included":
      return { tone: "ok", word: `Included in block ${o.block.toLocaleString("en-US")}`, line: success };
    case "reverted":
      return {
        tone: "danger",
        word: `Included in block ${o.block.toLocaleString("en-US")} · reverted`,
        line:
          `Receipt status 0: it didn't happen, and the fee was charged (${o.feeWei === null ? "fee not read" : saltFromWei(o.feeWei, 4) + " SALT"}).` +
          (o.reason ? ` Reason: ${o.reason}.` : ""),
      };
    case "not-sent":
      return { tone: "danger", word: "Not sent", line: `The node refused it before it entered the pool: ${o.reason}. No fee was charged.` };
    case "unknown":
      return {
        tone: "warn",
        word: o.txHash ? `Sent, outcome unknown: check tx ${shortHex(o.txHash, 6, 4)}` : "Sent, outcome unknown",
        line: "The app cannot prove the network did not accept this. Check the transaction before trying again. Nothing is re-sent automatically.",
      };
  }
}

/** A broadcast rejection as an outcome ("not submitted: …" is not sent; anything else is unknown). */
export function outcomeFromBroadcastError(err: unknown): TxOutcome {
  const f = broadcastFailure(err);
  return f.kind === "not-sent" ? { kind: "not-sent", reason: f.reason } : { kind: "unknown", txHash: f.txHash, reason: f.reason };
}

/** A receipt as an outcome: status 1 included, status 0 reverted with fee = gasUsed × effectiveGasPrice. */
export function outcomeFromReceipt(r: { status: number | null; blockNumber: number; gasUsed?: bigint | null; effectiveGasPrice?: bigint | null; reason?: string | null }): TxOutcome {
  if (r.status === 1) return { kind: "included", block: r.blockNumber };
  const fee = r.gasUsed != null && r.effectiveGasPrice != null ? r.gasUsed * r.effectiveGasPrice : null;
  return { kind: "reverted", block: r.blockNumber, feeWei: fee, reason: r.reason ?? null };
}

export type SheetStep = "review" | "signing" | "done";

export const SIGNING_STEPS = ["Reviewed by you", "Signed on this device", "Sent to the network, waiting for a receipt"] as const;

export interface CeremonySheetProps {
  id: string;
  title: string;
  hic: HicLevel;
  step: SheetStep;
  /** Signing steps completed so far (0..3) while `step` is "signing". */
  signed?: number;
  /** What the member signs, shown on review. */
  review: ReactNode;
  outcome?: TxOutcome;
  successLine?: string;
  signLabel: string;
  onSign: () => void;
  onDecline: () => void;
  /** Extra outcome actions ("Open the live round", "Check the transaction", "Review again"). */
  outcomeActions?: ReactNode;
  register?: "charter" | "instrument";
}

const TONE_VAR = { ok: "var(--ok)", warn: "var(--warn)", danger: "var(--danger)" } as const;
const TONE_BG = { ok: "var(--ok-bg)", warn: "var(--warn-bg)", danger: "var(--danger-bg)" } as const;

export function CeremonySheet(p: CeremonySheetProps) {
  const head = `${p.id}-h`;
  const words = p.step === "done" && p.outcome ? outcomeWords(p.outcome, p.successLine) : null;
  return (
    <ModalDialog
      labelledBy={head}
      focusKey={p.step}
      register={p.register ?? "charter"}
      zIndex={60}
      onEscape={p.step === "signing" ? undefined : p.onDecline}
      panelStyle={{ width: "min(600px, 100%)", maxHeight: "calc(100vh - 48px)", display: "flex", flexDirection: "column", background: "var(--srf-2)", color: "var(--tx-1)", border: "1px solid var(--line-2)", borderRadius: 8, overflow: "hidden" }}
    >
      <div style={{ flex: "none", display: "flex", alignItems: "center", gap: 10, padding: "12px 18px", borderBottom: "2px solid var(--line-strong)" }}>
        <HicBadge level={p.hic} />
        <h2 id={head} style={{ margin: 0, fontSize: 15, fontWeight: 600 }}>{p.title}</h2>
        <span style={{ flex: 1 }} />
        <span className="mono fl-faint" style={{ fontSize: 10.5 }}>signature ceremony</span>
      </div>
      <div data-testid="ceremony-body" data-step={p.step} style={{ padding: "14px 18px", display: "flex", flexDirection: "column", gap: 12, overflow: "auto" }}>
        {p.step === "review" && p.review}
        {p.step === "signing" && (
          <ol role="status" aria-live="polite" style={{ listStyle: "none", margin: 0, padding: "12px 0", display: "flex", flexDirection: "column", gap: 10, fontSize: 13 }}>
            {SIGNING_STEPS.map((s, i) => {
              const done = i < (p.signed ?? 0);
              return (
                <li key={s} data-done={done}>
                  <span aria-hidden="true">{done ? "✓" : "●"}</span> {s}
                  <span className="sr-only">{done ? ", done" : ", in progress"}</span>
                </li>
              );
            })}
          </ol>
        )}
        {words && (
          <div role="status" data-tone={words.tone} className="fl-card" style={{ padding: "12px 14px", borderColor: TONE_VAR[words.tone], background: TONE_BG[words.tone], display: "flex", flexDirection: "column", gap: 4 }}>
            <b style={{ fontWeight: 600, color: TONE_VAR[words.tone] }}>{words.word}</b>
            <span style={{ fontSize: 12.5 }}>{words.line}</span>
          </div>
        )}
      </div>
      <div style={{ flex: "none", display: "flex", gap: 8, justifyContent: "flex-end", padding: "12px 18px", borderTop: "1px solid var(--line-1)" }}>
        {p.step === "review" && (
          <>
            <button type="button" className="btn btn-ghost" data-autofocus onClick={p.onDecline}>
              Decline
            </button>
            <button type="button" className="btn btn-primary" onClick={p.onSign}>
              {p.signLabel}
            </button>
          </>
        )}
        {p.step === "done" && (
          <>
            <button type="button" className="btn btn-ghost" data-autofocus onClick={p.onDecline}>
              Close
            </button>
            {p.outcomeActions}
          </>
        )}
      </div>
    </ModalDialog>
  );
}
