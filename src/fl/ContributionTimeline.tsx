// =====================================================================
// citrate-core: your contribution timeline (FL design board 12, "Contribution timeline")
//
// Five steps for your lease: dataset version hash, training, model update hash and size, upload,
// verified receipt. Each step is pending or done; a failure names its step. A step ticks only on
// a local worker log line or a coordinator receipt (the caller passes the new step list).
// =====================================================================
export const CONTRIBUTION_STEPS = ["dataset", "training", "update", "upload", "receipt"] as const;
export type ContributionStepKey = (typeof CONTRIBUTION_STEPS)[number];

export const CONTRIBUTION_WORD: Record<ContributionStepKey, string> = {
  dataset: "Dataset version",
  training: "Training",
  update: "Model update",
  upload: "Upload",
  receipt: "Verified receipt",
};

export interface ContributionStep {
  key: ContributionStepKey;
  state: "pending" | "done" | "failed";
  /** "support-tickets v3 · 0x9c1e…", "1 h 42 min", "14 MB", "receipt 8841", or the failure. */
  detail?: string;
}

const GLYPH = { pending: "○", done: "✓", failed: "⊘" } as const;
const COLOR = { pending: "var(--tx-3)", done: "var(--ok)", failed: "var(--danger)" } as const;
const WORD = { pending: "pending", done: "done", failed: "failed" } as const;

export function ContributionTimeline({ steps }: { steps: ContributionStep[] }) {
  const by = new Map(steps.map((s) => [s.key, s]));
  return (
    <ol data-testid="contribution-timeline" aria-label="Your lease" style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 0 }}>
      {CONTRIBUTION_STEPS.map((k, i) => {
        const s = by.get(k) ?? { key: k, state: "pending" as const };
        return (
          <li key={k} data-step={k} data-state={s.state} style={{ display: "flex", gap: 10, alignItems: "baseline", padding: "7px 0", borderTop: i === 0 ? "none" : "1px solid var(--line-1)" }}>
            <span aria-hidden="true" style={{ color: COLOR[s.state], width: 14, textAlign: "center" }}>{GLYPH[s.state]}</span>
            <span style={{ fontSize: 13, minWidth: 130 }}>{CONTRIBUTION_WORD[k]}</span>
            <span className="sr-only">{WORD[s.state]}</span>
            <span className={s.state === "failed" ? undefined : "fl-muted"} style={{ fontSize: 12.5, color: s.state === "failed" ? "var(--danger)" : undefined, minWidth: 0, overflowWrap: "anywhere" }}>{s.detail ?? ""}</span>
          </li>
        );
      })}
    </ol>
  );
}
