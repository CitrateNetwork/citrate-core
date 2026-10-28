// =====================================================================
// citrate-core: Node › Advanced (core plan K3 + K6; design slice 2 "Node advanced")
//
// Data sources (Rule 7):
//   - network identity: eth_getBlockByNumber(0x0).hash on the profile's RPC against the genesis of
//     this build's active network profile (networkCompat.ts, K6); "Update required" on a definite
//     mismatch or an unset profile only;
//   - withdrawn branch: node_status.notice, set for this session by node_withdrawn.rs (or by the
//     genesis reset in node_genesis.rs) when the node's chain data was moved aside;
//   - crash records: node_last_crash (the last record; a list is not exposed yet, N-8);
//   - full log: node_logs, folded into AppState by Store.refreshNode (`s.logs`);
//   - configuration: the existing Node configuration section (config_read / config_write).
// =====================================================================
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { bridge } from "../../bridge";
import { BRIDGE_MODE } from "../../bridge/mode";
import type { NodeStatusView as NodeStatusDto } from "../../bridge/domains";
import { citrate } from "../../chain";
import { NODE_SETTINGS, sectionFor } from "../../shell/places";
import { useNetworkCompat, type NetworkCompat } from "../../shell/networkCompat";
import type { Store } from "../../shell/store";
import type { AppState } from "../../shell/state";
import { Settings } from "../Settings";
import { BoundedLog, PAGE, Panel, Row, StateCard, errText, logCaption } from "./parts";

/** The two reads this tab makes itself (the log comes from the store's poll). */
export interface AdvancedReads {
  nodeStatus(): Promise<NodeStatusDto>;
  lastCrash(): Promise<{ reason: string; atUnixMs: number } | null>;
}

const short = (h: string) => h.slice(0, 6) + "…" + h.slice(-4);
const utc = (ms: number) => new Date(ms).toISOString().slice(0, 16).replace("T", " ") + " UTC";

type Read<T> = { state: "reading" } | { state: "ok"; value: T } | { state: "error"; error: string };

const LIVE_READS: AdvancedReads = { nodeStatus: () => bridge.node.status(), lastCrash: () => bridge.node.lastCrash() };

/** Node › Advanced with its live reads and the existing configuration section. */
export function NodeAdvanced({ store, s }: { store: Store; s: AppState }) {
  const { status, live, expected } = useNetworkCompat();
  return (
    <NodeAdvancedView
      s={s}
      mode={BRIDGE_MODE}
      compat={status}
      liveGenesis={live}
      expectedGenesis={expected}
      reads={LIVE_READS}
      config={<Settings store={store} s={{ ...s, sSec: sectionFor(NODE_SETTINGS, s.sSec) }} sections={NODE_SETTINGS} heading="Configuration" />}
    />
  );
}

export function NodeAdvancedView({
  s,
  mode,
  compat,
  liveGenesis,
  expectedGenesis,
  reads,
  config,
}: {
  s: AppState;
  mode: "tauri" | "sim";
  compat: NetworkCompat;
  liveGenesis: string | null;
  expectedGenesis: string | null;
  reads: AdvancedReads;
  config?: ReactNode;
}) {
  const [notice, setNotice] = useState<Read<string | null>>({ state: "reading" });
  const [crash, setCrash] = useState<Read<{ reason: string; atUnixMs: number } | null>>({ state: "reading" });

  const load = useCallback(() => {
    reads
      .nodeStatus()
      .then((st) => setNotice({ state: "ok", value: st.notice ?? null }))
      .catch((e) => setNotice({ state: "error", error: errText(e) }));
    reads
      .lastCrash()
      .then((c) => setCrash({ state: "ok", value: c }))
      .catch((e) => setCrash({ state: "error", error: errText(e) }));
  }, [reads]);

  useEffect(() => {
    if (mode === "tauri") load();
  }, [mode, load]);

  if (mode !== "tauri") {
    return (
      <div data-node-tab="advanced" style={PAGE}>
        <StateCard
          kind="unavailable"
          title="Advanced needs the desktop app"
          body="The withdrawn-branch notice, the crash record and the node's log come from the app's supervisor on this machine. The web preview has none, so nothing is shown."
          why="node_status / node_last_crash / node_logs → Unavailable (BRIDGE_MODE = sim)"
        />
        {config}
      </div>
    );
  }

  const genRed = compat === "retired" || compat === "unset";
  const genWord = compat === "compatible" ? "matches this build" : genRed ? "Update required" : "not checked yet · —";

  return (
    <div data-node-tab="advanced" style={PAGE}>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(340px, 1fr))", gap: 16, alignItems: "start", maxWidth: 1160 }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 14, minWidth: 0 }}>
          <div className="surface" style={{ display: "flex", flexDirection: "column", borderColor: genRed ? "var(--danger)" : undefined }}>
            <div style={{ padding: "13px 18px", borderBottom: "1px solid var(--line-1)", display: "flex", flexDirection: "column", gap: 4 }}>
              <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span style={{ fontSize: 14, fontWeight: 500, flex: 1 }}>Network identity</span>
                <span
                  data-testid="genesis-word"
                  className="mono"
                  style={{ fontSize: 9.5, letterSpacing: ".08em", textTransform: "uppercase", padding: "2px 7px", borderRadius: 999, border: "1px solid " + (genRed ? "var(--danger)" : "var(--line-2)"), color: genRed ? "var(--danger)" : "var(--tx-2)" }}
                >
                  {genWord}
                </span>
              </span>
              <span style={{ fontSize: 12.5, lineHeight: 1.55, color: "var(--tx-2)" }}>
                {compat === "unset"
                  ? "This build's network profile is not complete, so it refuses to start a node or sign anything. An update ships the profile; your keys, wallet and settings stay."
                  : compat === "retired"
                    ? "The network's genesis no longer matches the one this build ships with, so this build is for a different chain. Update Citrate; your keys, wallet and settings stay."
                    : "Before trusting any read, the app checks the chain id and the genesis hash of its network profile."}
              </span>
            </div>
            <Row k="Chain id · expected" v={"0x" + citrate.id.toString(16)} src="this build's network profile (expected)" />
            <Row
              k="Genesis · expected"
              v={expectedGenesis ? short(expectedGenesis) : "— · not set in this build's profile"}
              vColor={expectedGenesis ? undefined : "var(--danger)"}
              src="the genesis hash in this build's active network profile"
            />
            <Row
              k="Genesis · on the network"
              v={liveGenesis ? short(liveGenesis) : "— · not read yet"}
              vColor={compat === "retired" ? "var(--danger)" : liveGenesis ? undefined : "var(--tx-3)"}
              src="eth_getBlockByNumber(0x0).hash on the profile's RPC"
            />
            <div style={{ padding: "12px 18px", display: "flex", flexDirection: "column", gap: 4 }}>
              <span style={{ fontSize: 13, fontWeight: 500 }}>Update required at genesis</span>
              <span style={{ fontSize: 12, lineHeight: 1.55, color: "var(--tx-2)" }}>
                Betanet starts from a new genesis. A build without the new genesis then shows Update required here and on Status. Updating keeps your keys, wallet and settings.
              </span>
            </div>
          </div>

          <Panel title="Withdrawn branch" caption="node_status.notice" testid="withdrawn">
            <div style={{ padding: "12px 18px", display: "flex", flexDirection: "column", gap: 8 }}>
              {notice.state === "reading" && <span style={{ fontSize: 12.5, color: "var(--tx-3)" }}>Reading…</span>}
              {notice.state === "error" && (
                <StateCard kind="error" title="Couldn't read the node's notice" body="Whether this session moved chain data aside is unknown here." why={"node_status → error: " + notice.error} cta={{ label: "Try again", go: load }} />
              )}
              {notice.state === "ok" && notice.value && (
                <>
                  <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".08em", textTransform: "uppercase", color: "var(--warn)" }}>
                    notice from this session
                  </span>
                  <span style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-1)", overflowWrap: "anywhere" }}>{notice.value}</span>
                </>
              )}
              {notice.state === "ok" && !notice.value && (
                <span style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-2)" }}>
                  No notice this session. If the network withdraws a branch this node followed, Citrate moves that chain data aside on the next start and syncs again; your keys, settings and wallet stay. The check runs once this node has applied each check height and its RPC returns that block, so “no notice” can also mean not checked yet.
                </span>
              )}
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                source: this node's own RPC at each compiled-in check height (node_withdrawn.rs)
              </span>
            </div>
          </Panel>

          <Panel title="Crash records" caption="the last record (node_last_crash); a list isn't exposed yet" testid="crash-record">
            <div style={{ padding: "12px 18px", display: "flex", flexDirection: "column", gap: 6 }}>
              {crash.state === "reading" && <span style={{ fontSize: 12.5, color: "var(--tx-3)" }}>Reading…</span>}
              {crash.state === "error" && (
                <StateCard kind="error" title="Couldn't read crash records" body="Whether the node crashed is unknown here, not “none”." why={"node_last_crash → error: " + crash.error} cta={{ label: "Try again", go: load }} />
              )}
              {crash.state === "ok" && !crash.value && <span style={{ fontSize: 12.5, color: "var(--tx-3)" }}>No crash recorded.</span>}
              {crash.state === "ok" && crash.value && (
                <>
                  <span className="mono" style={{ fontSize: 11.5 }}>{utc(crash.value.atUnixMs)} · node</span>
                  <span style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)", overflowWrap: "anywhere" }}>{crash.value.reason}</span>
                </>
              )}
            </div>
          </Panel>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 14, minWidth: 0 }}>
          <Panel title="Full log · node" caption={logCaption(s.logs.length)}>
            <div style={{ margin: "12px 18px 14px" }}>
              <BoundedLog testid="full-log" lines={s.logs} empty={s.node === "off" ? "The node is off, so there is no output." : "No output yet."} />
            </div>
            <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", padding: "0 18px 12px" }}>
              source: node_logs (~/.citrate/core/node.log). The other services' logs are not available yet.
            </span>
          </Panel>
        </div>
      </div>
      {config}
    </div>
  );
}
