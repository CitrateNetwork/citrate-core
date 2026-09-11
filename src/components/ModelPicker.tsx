// Hermes ModelRouter — the model picker (P0 / WP0.3).
//
// Presentational + source-of-truth-agnostic: it renders the router's ModelChoice[] and
// reports selections via onSelect. The mounting surface feeds it the LIVE choices (the
// Models section's local models + gateway + registry, via modelRouterSources) and the
// current activeId, so the picker and the Models section share one source of truth (wired
// in WP0.4). Honest states only (Rule 1): a not-ready local/registry choice is labeled as
// such — selecting it triggers the real download/pull; the gateway is the always-ready
// default when nothing is selected.
import type { CSSProperties } from "react";
import type { ModelChoice } from "../agent/modelRouter";

const sourceLabel: Record<ModelChoice["source"], string> = {
  local: "on-device",
  registry: "registry",
  gateway: "gateway",
};

/** Is this choice the effective selection? Null active ⇒ the gateway is the default. */
export function isSelectedChoice(c: ModelChoice, activeId: string | null): boolean {
  if (activeId == null) return c.source === "gateway";
  return c.id === activeId;
}

export function ModelPicker({
  choices,
  activeId,
  onSelect,
}: {
  choices: ModelChoice[];
  activeId: string | null;
  onSelect: (id: string) => void;
}) {
  const row: CSSProperties = {
    display: "flex",
    alignItems: "center",
    gap: 10,
    width: "100%",
    padding: "9px 12px",
    background: "none",
    border: "none",
    borderBottom: "1px solid var(--line-1)",
    cursor: "pointer",
    textAlign: "left",
    color: "var(--tx-1)",
  };
  return (
    <div role="listbox" aria-label="Model" data-testid="model-picker" style={{ border: "1px solid var(--line-1)", borderRadius: "var(--r-2)", overflow: "hidden", background: "var(--srf-1)" }}>
      {choices.map((c) => {
        const selected = isSelectedChoice(c, activeId);
        // A local/registry choice that isn't ready yet — selecting it starts the real work.
        const notReady = !c.ready && c.source !== "gateway";
        const hint = notReady ? (c.source === "registry" ? "pull to use" : "download to use") : sourceLabel[c.source];
        return (
          <button
            key={c.id}
            role="option"
            aria-selected={selected}
            data-source={c.source}
            data-ready={c.ready}
            onClick={() => onSelect(c.id)}
            style={{ ...row, background: selected ? "var(--srf-2)" : "none" }}
          >
            <span style={{ width: 6, height: 6, borderRadius: 999, flexShrink: 0, background: c.ready ? "var(--accent)" : "var(--tx-3)" }} aria-hidden></span>
            <span style={{ flex: 1, minWidth: 0, fontSize: 13, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{c.label}</span>
            <span className="mono" style={{ fontSize: 10, letterSpacing: ".06em", textTransform: "uppercase", color: notReady ? "var(--warn)" : "var(--tx-3)" }}>{hint}</span>
            {selected && (
              <span aria-hidden style={{ color: "var(--accent-text)", fontSize: 12 }}>✓</span>
            )}
          </button>
        );
      })}
    </div>
  );
}
