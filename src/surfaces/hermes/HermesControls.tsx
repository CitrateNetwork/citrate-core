// =====================================================================
// K2 (0.5.2): Hermes › Controls at three depths (design slice 1). The depth is the route; the place
// tab strip shows it as the Controls sub-tabs.
//
//   hermes/controls           1 · Switches  ControlsPanel: six switches, each with Details ›
//   hermes/controls/details   2 · Details   the Settings sections for Hermes, unchanged components:
//                                           Providers & keys, Escalation & spend caps, MCP servers,
//                                           Web search & decisions, Terminal & folders, Budgets,
//                                           Personas & voice (CONTROLS_DETAIL_SETTINGS)
//   hermes/controls/records   3 · Records   ControlsRecords: approval records, spend ledger, runs
//
// No form is duplicated: depth 2 mounts the same section components Settings did, and each writer
// re-reads before it writes (read-modify-write, #341/#343; webSettingsSync for web search).
// =====================================================================
import type { Store } from "../../shell/store";
import type { AppState } from "../../shell/state";
import { CONTROLS_DETAIL_SETTINGS, sectionFor } from "../../shell/places";
import { Settings } from "../Settings";
import { CONTROL_DETAILS, ControlsPanel } from "./ControlsPanel";
import { ControlsRecords } from "./ControlsRecords";

const HINT: Record<string, string> = {
  "hermes/controls": "Six switches anyone can read.",
  "hermes/controls/details": "Providers, spend caps, MCP servers, web search, terminal, budgets and personas.",
  "hermes/controls/records": "What happened: approvals, spend and every daemon run.",
};

export function HermesControls({ route, store, s }: { route: string; store: Store; s: AppState }) {
  const hint = <span style={{ fontSize: 12, color: "var(--tx-3)" }}>{HINT[route] ?? HINT["hermes/controls"]}</span>;
  if (route === "hermes/controls/details") {
    return (
      <>
        <div style={{ padding: "16px 26px 0" }}>{hint}</div>
        <Settings store={store} s={{ ...s, sSec: sectionFor(CONTROLS_DETAIL_SETTINGS, s.sSec) }} sections={CONTROLS_DETAIL_SETTINGS} heading="Details" />
      </>
    );
  }
  if (route === "hermes/controls/records") {
    return (
      <div style={{ padding: "16px 26px 24px", display: "flex", flexDirection: "column", gap: 14 }}>
        {hint}
        <ControlsRecords />
      </div>
    );
  }
  return (
    <div style={{ padding: "16px 26px 24px", display: "flex", flexDirection: "column", gap: 14, maxWidth: 1000 }}>
      {hint}
      <ControlsPanel
        store={store}
        s={s}
        onDetails={(id) => {
          const d = CONTROL_DETAILS[id];
          if (d.sSec) store.setState({ sSec: d.sSec });
          store.go(d.route);
        }}
      />
    </div>
  );
}
