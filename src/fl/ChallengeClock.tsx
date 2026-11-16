// =====================================================================
// citrate-core: the challenge clock (FL design board 12, "Challenge clock")
//
// FederatedRoundLedger's challenge window as a bar from the commit block to the window-end block.
// The sentence carries the state; the bar is secondary. The fill steps forward only when a new
// head block arrives (no tween between blocks). The duration is said only from a measured block
// time; without one the clock says blocks only. Source: FRL round deadline + eth_blockNumber.
// =====================================================================
import { aboutDuration, blockText } from "./format";

export type ClockState = "not-open" | "open" | "closed" | "ended-early";

export interface ChallengeClockProps {
  state: ClockState;
  commitBlock?: number;
  endBlock?: number;
  /** The node's head block, or null when it was not read. */
  head: number | null;
  /** Seconds per block measured from recent blocks, or null when not measured. */
  blockSeconds: number | null;
}

/** The clock's sentence (also its text equivalent). */
export function clockSentence({ state, endBlock, head, blockSeconds }: ChallengeClockProps): string {
  if (state === "ended-early") return "Challenge window ended early: a challenge was proven";
  if (state === "not-open" || endBlock === undefined) return "The challenge window opens when the merge is committed";
  if (state === "closed") return `Challenge window closed at block ${blockText(endBlock)} with no successful challenge`;
  if (head === null) return `Challenge window ends at block ${blockText(endBlock)}; the current block was not read`;
  const left = Math.max(0, endBlock - head);
  const when = blockSeconds !== null && blockSeconds > 0 ? `, in ${aboutDuration(left * blockSeconds)}` : `, ${blockText(left)} blocks from now`;
  return `Challenge window ends at block ${blockText(endBlock)}${when}`;
}

/** Fraction of the window passed, 0..1, or null when it cannot be drawn. */
export function clockFraction({ state, commitBlock, endBlock, head }: ChallengeClockProps): number | null {
  if (state === "closed" || state === "ended-early") return 1;
  if (state !== "open" || commitBlock === undefined || endBlock === undefined || head === null || endBlock <= commitBlock) return null;
  return Math.min(1, Math.max(0, (head - commitBlock) / (endBlock - commitBlock)));
}

export function ChallengeClock(p: ChallengeClockProps) {
  const sentence = clockSentence(p);
  const f = clockFraction(p);
  const color = p.state === "ended-early" ? "var(--danger)" : p.state === "closed" ? "var(--ok)" : "var(--accent)";
  return (
    <div data-testid="challenge-clock" data-state={p.state} style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <span style={{ fontSize: 13 }}>{sentence}</span>
      {f !== null && (
        <div role="img" aria-label={sentence} className="fl-inset" style={{ height: 8, overflow: "hidden" }}>
          <div style={{ width: `${(f * 100).toFixed(1)}%`, height: "100%", background: color }} />
        </div>
      )}
      {p.commitBlock !== undefined && p.endBlock !== undefined && (
        <span className="mono fl-faint" style={{ fontSize: 11 }}>
          block {blockText(p.commitBlock)} to {blockText(p.endBlock)}
        </span>
      )}
    </div>
  );
}
