// =====================================================================
// citrate-core — onboarding hardware tier (HUP-S1.6, US-1.6)
//
// AC1: the local probe runs at onboarding and the tier + its rationale are shown.
// AC2: the user can override; the choice persists (Rust `tier_set_override` → config.json).
//
// `TierView` is pure (static-render tested); `TierPanel` wires it to the tier slice. Anything the
// probe could not read is shown as unknown (Rule 1). Nothing is downloaded from here: the Gemma
// download in the model step stays the default flow.
// =====================================================================
import { useEffect, type CSSProperties } from "react";
import type { HardwareFacts, TierId, TierReport } from "../bridge/domains";
import { tierSlice, refreshTier, setTierOverride, effectiveProfile } from "../shell/slices/tier";

const GIB = 2 ** 30;

/** GiB, truncated to one decimal so a value just under a boundary never reads as the boundary. */
function gb(bytes: number): string {
  const tenths = Math.floor((bytes * 10) / GIB);
  return tenths % 10 === 0 ? String(tenths / 10) : (tenths / 10).toFixed(1);
}

function memoryLine(f: HardwareFacts): string {
  const mem = f.totalRamBytes == null ? "memory unknown" : `${gb(f.totalRamBytes)} GB ${f.unifiedMemory ? "unified memory" : "memory"}`;
  const gpu = f.unifiedMemory
    ? "GPU shares it"
    : f.gpuVramBytes != null
      ? `${gb(f.gpuVramBytes)} GB GPU memory`
      : "GPU memory unknown";
  return `${mem} · ${gpu}`;
}

const ctxLabel = (n: number) => `${Math.round(n / 1024)}k context`;

const note: CSSProperties = { fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 };

export function TierView({
  report,
  loaded,
  saving,
  error,
  onOverride,
}: {
  report: TierReport | null;
  loaded: boolean;
  saving: boolean;
  error: string | null;
  onOverride: (tier: TierId | null) => void;
}) {
  if (!report) {
    return (
      <div className="mono" style={note} data-testid="tier-panel">
        {!loaded
          ? "Checking this machine…"
          : error
            ? `Hardware check failed: ${error}`
            : "Hardware tier: the hardware check needs the desktop app."}
      </div>
    );
  }
  const rec = report.recommendation;
  const eff = effectiveProfile(report);
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, borderTop: "1px solid var(--line-1)", paddingTop: 12 }} data-testid="tier-panel">
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 12, flexWrap: "wrap" }}>
        <div style={{ fontSize: 14, fontWeight: 500 }} data-testid="tier-summary">
          Your machine: {rec.tier} — {memoryLine(report.facts)}
        </div>
        <label className="mono" style={{ fontSize: 11, color: "var(--tx-3)", display: "flex", alignItems: "center", gap: 6 }}>
          tier
          <select
            data-testid="tier-override"
            value={report.overrideTier ?? ""}
            disabled={saving}
            onChange={(e) => onOverride(e.target.value === "" ? null : (e.target.value as TierId))}
            style={{ fontSize: 12, padding: "3px 6px" }}
          >
            <option value="">Recommended ({rec.tier})</option>
            {report.profiles.map((p) => (
              <option key={p.tier} value={p.tier}>
                {p.tier} — {p.modelHint}
              </option>
            ))}
          </select>
        </label>
      </div>
      <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12.5, color: "var(--tx-2)", lineHeight: 1.6 }} data-testid="tier-rationale">
        {rec.rationale.map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
      {eff && (
        <div className="mono" style={{ fontSize: 11.5, color: "var(--tx-2)" }} data-testid="tier-effective">
          {report.overrideTier ? `Using ${report.effective} (your choice)` : `Using ${report.effective}`} · {eff.modelHint} · {ctxLabel(eff.ctxTokens)}
        </div>
      )}
      {(rec.guided || report.effective === "T0") && (
        <div style={note}>
          T0 is the guided tier: a small model runs here, and bigger planning jobs are best escalated to a larger model you choose.
        </div>
      )}
      <div style={note}>
        Model picks per tier are provisional until the tier evaluation. Nothing extra is downloaded — the model below stays the default
        unless you pick another in Models.
      </div>
      {error && (
        <div className="mono" style={{ fontSize: 11.5, color: "var(--danger)" }}>
          {error}
        </div>
      )}
    </div>
  );
}

/** Runs the local probe once on mount and renders the result with a persisted override. */
export function TierPanel() {
  const st = tierSlice.use();
  useEffect(() => {
    void refreshTier();
  }, []);
  return <TierView report={st.report} loaded={st.loaded} saving={st.saving} error={st.error} onOverride={(t) => void setTierOverride(t)} />;
}
