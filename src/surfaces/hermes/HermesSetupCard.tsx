// 0.5.4: the Hermes setup self-check on the Agent page. Data: hermesSetupSlice
// (hermes_setup_check + hermes_status + the app's own tool list and prompt). Read on mount and
// on "Check again"; every row says ok, missing (with why) or off (turned off by the member).
import { useEffect, useState } from "react";
import { hermesSetupSlice, offerNodeTools, offerRepair, runHermesSelfCheck, runSetupAction, setupSummary, type SetupItem } from "../../shell/slices/hermesSetup";
import { agentSlice } from "../../shell/slices/agent";

const TONE: Record<SetupItem["state"], { fg: string; word: string }> = {
  ok: { fg: "var(--ok)", word: "ok" },
  missing: { fg: "var(--danger)", word: "missing" },
  off: { fg: "var(--tx-3)", word: "off" },
  unchecked: { fg: "var(--warn)", word: "not checked" },
};

export function HermesSetupCard() {
  const s = hermesSetupSlice.use();
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const act = (cmd: "hermes_setup_repair" | "hermes_setup_enable_node_tools") => {
    setBusy(true);
    setActionError(null);
    void runSetupAction(cmd).then((e) => {
      setBusy(false);
      setActionError(e);
    });
  };
  // Checked again each time Hermes starts or stops (what it loaded is read only while it runs).
  const hermesState = agentSlice.use().attached.find((r) => r.id === "hermes")?.state;
  useEffect(() => {
    if (hermesState === "starting") return;
    void runHermesSelfCheck();
  }, [hermesState]);
  return (
    <div className="surface" style={{ display: "flex", flexDirection: "column" }} aria-label="Hermes setup">
      <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)", gap: 10 }}>
        <span style={{ fontSize: 13.5, fontWeight: 500 }}>Setup</span>
        <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: s.missingRequired > 0 ? "var(--danger)" : s.complete ? "var(--ok)" : "var(--tx-3)" }}>
          {setupSummary(s)}
        </span>
        <button className="btn btn-ghost btn-sm" style={{ marginLeft: "auto" }} onClick={() => void runHermesSelfCheck()}>
          Check again
        </button>
      </div>
      {s.error && (
        <p style={{ fontSize: 12.5, color: "var(--danger)", margin: 0, padding: "10px 16px" }}>The setup report could not be read: {s.error}</p>
      )}
      {(s.items ?? []).map((i) => (
        <div key={i.id} style={{ display: "flex", alignItems: "baseline", gap: 12, padding: "9px 16px", borderBottom: "1px solid var(--line-1)" }}>
          <span className="mono" style={{ width: 58, fontSize: 10.5, color: TONE[i.state].fg, flexShrink: 0 }}>{TONE[i.state].word}</span>
          <span style={{ flex: 1, minWidth: 0 }}>
            <span style={{ display: "block", fontSize: 12.5 }}>
              {i.label}
              {i.need === "optional" && <span style={{ color: "var(--tx-3)" }}> · optional</span>}
            </span>
            <span style={{ display: "block", fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 }}>{i.detail}</span>
          </span>
        </div>
      ))}
      {(offerNodeTools(s) || offerRepair(s) || actionError) && (
        <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 8, padding: "10px 16px" }}>
          {offerNodeTools(s) && (
            <button className="btn btn-primary btn-sm" disabled={busy} onClick={() => act("hermes_setup_enable_node_tools")}>
              Turn on node tools
            </button>
          )}
          {offerRepair(s) && (
            <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => act("hermes_setup_repair")}>
              Repair setup
            </button>
          )}
          {offerNodeTools(s) && (
            <span style={{ fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 }}>
              Turns on this node's MCP server (127.0.0.1 only) and offers Hermes its read tools, from Hermes's next start.
            </span>
          )}
          {actionError && <span style={{ fontSize: 12, color: "var(--danger)" }}>{actionError}</span>}
        </div>
      )}
      {s.firstRunDefaults.length > 0 && (
        <p style={{ fontSize: 11.5, color: "var(--tx-3)", margin: 0, padding: "10px 16px", lineHeight: 1.5 }}>
          Set up at this launch: {s.firstRunDefaults.join("; ")}. You can change these in Hermes › Controls and Settings.
        </p>
      )}
    </div>
  );
}
