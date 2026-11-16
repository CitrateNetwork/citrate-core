// =====================================================================
// citrate-core: the round ring (FL design board 12, "Round ring")
//
// Five labelled segments: Open, Running, Verifying, Payout, Closed. Passed segments use
// --fl-ring-done, the current one is wider and uses --fl-ring-current, the rest the track.
// Disputed marks the Payout segment, Rejected marks Verifying, Expired greys every segment.
// Motion: a segment's stroke changes only when the caller passes a new stage (WorkMarket and
// FederatedRoundLedger events); reduced motion makes it instant (fl.css). Colour is never alone:
// the current segment is wider and the legend names each stage. The whole ring is one role="img"
// with the caller's full sentence as its label.
// =====================================================================
export const RING_STAGES = ["open", "running", "verifying", "payout", "closed"] as const;
export type RingStage = (typeof RING_STAGES)[number];
export type RingOutcome = "disputed" | "rejected" | "expired";

export const STAGE_WORD: Record<RingStage, string> = {
  open: "Open",
  running: "Running",
  verifying: "Verifying",
  payout: "Payout",
  closed: "Closed",
};

export interface RoundRingProps {
  stage: RingStage;
  outcome?: RingOutcome;
  /** The text equivalent, a full sentence: "Round 14: verifying, 6 of 8 leases verified, …". */
  label: string;
  /** Pixel size of the square; 48 for a rail card, 200 for the live round. */
  size?: number;
  legend?: boolean;
}

export type SegmentPaint = "track" | "done" | "current" | "disputed" | "rejected" | "expired";

/** How each segment is painted for a stage and outcome (pure, for the view and its tests). */
export function ringPaint(stage: RingStage, outcome?: RingOutcome): SegmentPaint[] {
  const at = RING_STAGES.indexOf(stage);
  return RING_STAGES.map((s, i) => {
    if (outcome === "expired") return "expired";
    if (outcome === "rejected" && s === "verifying") return "rejected";
    if (outcome === "disputed" && s === "payout") return "disputed";
    if (i < at) return "done";
    if (i === at) return "current";
    return "track";
  });
}

const PAINT_VAR: Record<SegmentPaint, string> = {
  track: "var(--fl-ring-track)",
  done: "var(--fl-ring-done)",
  current: "var(--fl-ring-current)",
  disputed: "var(--fl-ring-disputed)",
  rejected: "var(--fl-lease-rejected)",
  expired: "var(--fl-ring-expired)",
};

const PAINT_WORD: Record<SegmentPaint, string> = {
  track: "not reached",
  done: "passed",
  current: "now",
  disputed: "disputed",
  rejected: "rejected",
  expired: "expired",
};

function arc(cx: number, cy: number, r: number, a0: number, a1: number): string {
  const p = (a: number) => [cx + r * Math.sin(a), cy - r * Math.cos(a)];
  const [x0, y0] = p(a0);
  const [x1, y1] = p(a1);
  const large = a1 - a0 > Math.PI ? 1 : 0;
  return `M ${x0.toFixed(2)} ${y0.toFixed(2)} A ${r} ${r} 0 ${large} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`;
}

export function RoundRing({ stage, outcome, label, size = 200, legend = false }: RoundRingProps) {
  const paint = ringPaint(stage, outcome);
  const c = size / 2;
  const thin = Math.max(3, size * 0.065);
  const wide = Math.max(5, size * 0.1);
  const r = c - wide / 2 - 1;
  const gap = 0.06;
  const seg = (Math.PI * 2) / RING_STAGES.length;
  return (
    <figure data-testid="round-ring" data-stage={stage} data-outcome={outcome ?? ""} style={{ margin: 0, display: "flex", gap: 14, alignItems: "center", flexWrap: "wrap" }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} role="img" aria-label={label} style={{ flexShrink: 0 }}>
        {RING_STAGES.map((s, i) => (
          <path
            key={s}
            className="fl-ring-seg"
            data-seg={s}
            data-paint={paint[i]}
            d={arc(c, c, r, i * seg + gap, (i + 1) * seg - gap)}
            fill="none"
            stroke={PAINT_VAR[paint[i]]}
            strokeWidth={paint[i] === "current" ? wide : thin}
            strokeLinecap="butt"
          />
        ))}
      </svg>
      {legend && (
        <ul aria-label="Round stages" style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 4, fontSize: 12 }}>
          {RING_STAGES.map((s, i) => (
            <li key={s} style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <span aria-hidden="true" style={{ width: 14, height: paint[i] === "current" ? 6 : 3, background: PAINT_VAR[paint[i]], borderRadius: 1 }} />
              <span style={{ color: paint[i] === "current" ? "var(--tx-1)" : "var(--tx-2)", fontWeight: paint[i] === "current" ? 500 : 400 }}>{STAGE_WORD[s]}</span>
              <span className="fl-faint" style={{ fontSize: 11 }}>{PAINT_WORD[paint[i]]}</span>
            </li>
          ))}
        </ul>
      )}
    </figure>
  );
}
