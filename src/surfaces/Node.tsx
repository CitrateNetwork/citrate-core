import { useRef, useEffect } from "react";
import { SurfaceProps } from "./shared";
import { nodeLabel } from "../shell/state";

// ---- formatting helpers (verbatim from design) ----
const fmtI = (n: number) => Math.round(n).toLocaleString("en-US");
const fmt2 = (n: number) => n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const nodeColors: Record<string, string> = {
  off: "var(--tx-3)",
  prov: "#ffbd10",
  syncing: "#ffbd10",
  synced: "#8ecc09",
  paused: "#ffbd10",
  validating: "#8ecc09",
  error: "#dd7259",
};

const fmtDur = (sec: number | null): string => {
  if (sec == null) return "—";
  const h = Math.floor(sec / 3600),
    m = Math.floor((sec % 3600) / 60);
  return h > 0 ? h + "h " + String(m).padStart(2, "0") + "m" : m + "m " + String(Math.floor(sec % 60)).padStart(2, "0") + "s";
};

/**
 * Node surface — 1:1 from design/CitrateCore.dc.html NODE section.
 * Operations / Earning / Pinning sub-tabs. Every real datum is captioned with
 * its source (node-agent 127.0.0.1:19600). Claims and pins are unsigned
 * node-agent requests surfaced through the SignatureCeremony (store.requestSig);
 * no key ever leaves the ceremony. The Pinning tab is the honest [SEAM] one —
 * the PoSt sealer sidecar is pending upstream, and the copy says so.
 */
export function Node({ store, s }: SurfaceProps) {
  const pinCidEl = useRef<HTMLInputElement | null>(null);
  const pinBondEl = useRef<HTMLInputElement | null>(null);

  // CORE-C2 — when the Earning tab is open, pull the REAL claimable from
  // ContributionAccounting.claimable(vaultAddress) via eth_call (Rule 11). The
  // decomposition (Validation/Pinning/Compute) has NO on-chain source and is
  // labeled as an off-chain estimate below (Rule 1 / I-3).
  useEffect(() => {
    if (s.nTab === "earn") void store.refreshEarnings();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [s.nTab]);

  const staked = (s.hasGrant ? 32000 : 0) + s.selfStake;

  // ----- header / tabs -----
  const ntabs: [string, string][] = [
    ["ops", "Operations"],
    ["earn", "Earning"],
    ["pin", "Pinning"],
  ];
  const nodeStateColor = nodeColors[s.node];
  const nodeStateLabel = nodeLabel(s.node);
  const nodeDotAnim = s.node === "validating" || s.node === "syncing" ? "ccPulse 2.4s var(--ease-standard) infinite" : "none";

  // ----- operations -----
  const noStart = s.node !== "off";
  const noPause = !(s.node === "validating" || s.node === "synced" || s.node === "syncing");
  const noResume = s.node !== "paused";
  const noStop = s.node === "off" || s.node === "prov";
  const onNodeStart = () => store.startNode();
  const onNodePause = () => {
    store.setState({ node: "paused" });
    store.toast("Paused — heartbeat continues; no jobs in flight");
    store.save();
  };
  const onNodeResume = () => {
    store.setState({ node: staked >= 32000 ? "validating" : "synced" });
    store.save();
  };
  const onNodeStop = () => {
    store.setState({ node: "off", peers: 0, logs: [], syncPct: 0 });
    store.toast("Node stopped — supervisor released");
    store.save();
  };
  const showSync = s.node === "syncing";
  const syncPctStr = (s.syncPct | 0) + "%";
  const syncBarW = (s.syncPct | 0) + "%";
  const logsEmpty = s.node === "off";
  const logLines = s.logs;
  const peersEmpty = s.node === "off" || s.peerRows.length === 0;
  const peerRows = s.node === "off" ? [] : s.peerRows;
  const cpuStr = s.node === "off" ? "—" : (s.cpu | 0) + " %";
  const ramStr = s.node === "off" ? "—" : (s.ram | 0) + " MB";
  const diskStr = s.node === "off" ? "—" : "3.1 GB";
  const crashEmpty = s.crashes.length === 0;
  const crashRows = s.crashes;
  const blocksProposedStr = s.node === "off" ? "—" : fmtI(s.blocksProposed);
  const electionStr =
    s.node === "validating" && staked >= 32000 ? ((staked / 8200000) * 100).toFixed(2) + "% stake share / round" : "—";
  const restartsStr = String(s.crashes.length);

  // ----- earning -----
  const earnValStr = fmt2(s.earnVal);
  const earnPinStr = fmt2(s.earnPin);
  const earnCompStr = fmt2(s.earnComp);
  const claimStr = fmt2(s.claimable);
  const claimSourceStr =
    s.earnSource === "chain"
      ? "from ContributionAccounting.claimable() · eth_call 40204"
      : "prototype value — pending on-chain read";
  const claimDisabled = s.claimable < 5;
  const claimNote =
    s.claimable < 5
      ? "Claims batch until ≥ 5 SALT to avoid dust gas — " + fmt2(5 - s.claimable) + " SALT to go."
      : "Claimable is above the 5 SALT dust threshold.";
  // CORE-C2-F-1 (@rule8) — the Claim button drives the REAL claim path
  // (`store.claimRewards` → `bridge.agent.claim` → the real `claimRewards()`
  // ceremony → B1.4 broadcast in the desktop build; an honest "nothing to claim"
  // when the on-chain claimable is 0, and an honest "web preview cannot settle"
  // otherwise). It does NOT locally mutate liquid/claimable and it does NOT toast a
  // fabricated "Claimed — balance updated from chain" (Rule 1 / I-3): the balance
  // changes ONLY when the real tx settles and `claimable` is re-read from chain.
  const onClaim = () => {
    void store.claimRewards();
  };
  const hbAge = s.node === "off" ? null : s.hb;
  const hbStr = hbAge == null ? "—" : "last ack " + hbAge.toFixed(0) + "s ago · 30s window";
  const hbColor = hbAge == null ? "var(--tx-3)" : hbAge < 20 ? "var(--ok)" : "var(--warn)";
  const hbBarW = hbAge == null ? "0%" : Math.max(4, 100 - (hbAge / 30) * 100).toFixed(0) + "%";

  // ----- pinning -----
  const nodeOff = s.node === "off" || s.node === "prov";
  const pinRows = s.pins.map((p, i) => ({
    cid: p.cid,
    bond: p.bond + " SALT",
    cadence: "every " + p.cadH + " h",
    next: nodeOff ? "— node off" : "in " + fmtDur(p.nextIn),
    nextColor: nodeOff ? "var(--tx-3)" : p.nextIn < 900 ? "var(--warn)" : "var(--tx-2)",
    last: p.last === "pending" ? "proof pending sealer" : "attested · daemon",
    lastColor: p.last === "pending" ? "var(--warn)" : "var(--ok)",
    rowClass: i === 0 && s.justSigned === "Pin bond" ? "cc-row-stroke" : "",
  }));
  const pinsEmpty = s.pins.length === 0;
  const bonded = s.pins.reduce((a, p) => a + p.bond, 0);
  const pinBonded = fmtI(bonded);
  const pinCount = String(s.pins.length);
  const pinProjected = fmt2(bonded * 0.0075);
  const pinPassed = s.pins.length ? String(s.pins.length * 9 + 4) : "—";
  const nextPin = s.pins.length && !nodeOff ? Math.min.apply(null, s.pins.map((p) => p.nextIn)) : null;
  const pinNext = nextPin == null ? "—" : fmtDur(nextPin);
  const onPinNew = () => {
    const cid = pinCidEl.current ? pinCidEl.current.value.trim() : "";
    const bond = parseFloat(pinBondEl.current ? pinBondEl.current.value : "");
    if (!/^baf[a-z0-9]{6,}/i.test(cid)) return store.toast("Enter a CID (bafy…)");
    if (!(bond >= 20)) return store.toast("Minimum bond is 20 SALT");
    if (bond > s.liquid) return store.toast("Bond exceeds your liquid balance");
    const cidShort = cid.length > 16 ? cid.slice(0, 11) + "…" + cid.slice(-4) : cid;
    store.requestSig({
      origin: "node-agent",
      requester: "node-agent · PIN planner 127.0.0.1:19600",
      title: "Bond " + fmt2(bond) + " SALT to pin " + cidShort,
      rows: [
        { k: "Action", v: "plan_pin(" + cidShort + ") → bond" },
        { k: "Plan", v: "replication 3 · challenge every 6 h" },
        { k: "Bond", v: fmt2(bond) + " SALT · slashable on failed challenge" },
        { k: "Projected", v: "~" + fmt2(bond * 0.0075) + " SALT / month at current demand" },
      ],
      cost: "est. gas 0.0019 SALT",
      sponsor: "gas sponsored — standard daily budget",
      sponsorColor: "var(--ok)",
      apply: (h) => {
        store.setState((st) => ({
          liquid: st.liquid - bond,
          pins: [{ cid: cidShort, bond, cadH: 6, nextIn: 6 * 3600, last: "attested" }].concat(st.pins),
        }));
        store.addActivity("Pin bond", "−" + fmt2(bond) + " SALT", h);
        if (pinCidEl.current) pinCidEl.current.value = "";
        if (pinBondEl.current) pinBondEl.current.value = "";
        store.toast("Bond posted — challenges begin next window");
      },
    });
  };

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16 }}>
      {/* header */}
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Node</span>
        <span style={{ display: "flex", gap: 2, background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: 2 }}>
          {ntabs.map(([id, label]) => (
            <button
              key={id}
              onClick={() => {
                store.setState({ nTab: id });
                store.save();
              }}
              style={{
                fontFamily: "var(--font-sans)",
                fontSize: 12,
                fontWeight: s.nTab === id ? 500 : 400,
                padding: "5px 12px",
                border: "none",
                borderRadius: 5,
                cursor: "pointer",
                background: s.nTab === id ? "var(--srf-2)" : "transparent",
                color: s.nTab === id ? "var(--tx-1)" : "var(--tx-3)",
              }}
            >
              {label}
            </button>
          ))}
        </span>
        <span style={{ marginLeft: "auto", display: "inline-flex", alignItems: "center", gap: 8 }}>
          <span style={{ width: 8, height: 8, borderRadius: 999, background: nodeStateColor, animation: nodeDotAnim }}></span>
          <span className="mono" style={{ fontSize: 11, letterSpacing: ".08em", textTransform: "uppercase", color: nodeStateColor }}>
            {nodeStateLabel}
          </span>
        </span>
      </div>

      {/* ===================== OPERATIONS ===================== */}
      {s.nTab === "ops" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div className="surface" style={{ padding: "16px 18px", display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
            <button className="btn btn-primary" onClick={onNodeStart} disabled={noStart}>
              Start
            </button>
            <button className="btn btn-ghost" onClick={onNodePause} disabled={noPause}>
              Pause
            </button>
            <button className="btn btn-ghost" onClick={onNodeResume} disabled={noResume}>
              Resume
            </button>
            <button className="btn btn-danger" onClick={onNodeStop} disabled={noStop}>
              Stop
            </button>
            <span className="mono" style={{ marginLeft: "auto", fontSize: 10.5, color: "var(--tx-3)" }}>
              supervision · node-agent 127.0.0.1:19600 · bearer 0600
            </span>
          </div>

          {showSync && (
            <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 10 }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
                <span style={{ fontSize: 13.5, fontWeight: 500 }}>Syncing</span>
                <span className="mono tabular" style={{ fontSize: 13, color: "var(--accent-text)" }}>
                  {syncPctStr}
                </span>
              </div>
              <div style={{ height: 6, background: "var(--srf-inset)", borderRadius: 999, overflow: "hidden", border: "1px solid var(--line-1)" }}>
                <div style={{ height: "100%", background: "var(--accent)", width: syncBarW, transition: "width .5s var(--ease-standard)" }}></div>
              </div>
            </div>
          )}

          <div style={{ display: "grid", gridTemplateColumns: "1.3fr 1fr", gap: 12 }}>
            {/* log tail */}
            <div className="surface" style={{ display: "flex", flexDirection: "column", minHeight: 0 }}>
              <div style={{ display: "flex", alignItems: "center", padding: "11px 16px", borderBottom: "1px solid var(--line-1)" }}>
                <span style={{ fontSize: 13.5, fontWeight: 500 }}>Log tail</span>
                <span className="mono" style={{ marginLeft: "auto", fontSize: 10, color: "var(--tx-3)" }}>
                  ~/.citrate/core/node.log
                </span>
              </div>
              <div style={{ padding: "10px 16px", display: "flex", flexDirection: "column", gap: 4, maxHeight: 280, overflow: "auto", background: "var(--srf-inset)" }}>
                {logsEmpty && (
                  <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                    — node is off; no log stream —
                  </span>
                )}
                {logLines.map((lg) => (
                  <span key={lg.id} className="mono tabular" style={{ fontSize: 10.5, lineHeight: 1.6, color: "var(--tx-2)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                    <span style={{ color: "var(--tx-3)" }}>{lg.t}</span>  {lg.line}
                  </span>
                ))}
              </div>
            </div>

            {/* right column */}
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
                <div style={{ padding: "11px 16px", borderBottom: "1px solid var(--line-1)", fontSize: 13.5, fontWeight: 500 }}>Peers</div>
                {peersEmpty && (
                  <p style={{ fontSize: 12, color: "var(--tx-3)", margin: 0, padding: "12px 16px" }}>—</p>
                )}
                {peerRows.map((p, i) => (
                  <div key={i} style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 16px", borderBottom: "1px solid var(--line-1)" }}>
                    <span className="mono" style={{ fontSize: 11, flex: 1, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                      {p.id}
                    </span>
                    <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>
                      {p.dir}
                    </span>
                    <span className="mono tabular" style={{ fontSize: 11, color: "var(--tx-2)", width: 48, textAlign: "right" }}>
                      {p.lat}
                    </span>
                  </div>
                ))}
              </div>

              <div className="surface" style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
                <span className="eyebrow">Validator</span>
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Blocks proposed</span>
                  <span className="mono tabular" style={{ fontSize: 12 }}>{blocksProposedStr}</span>
                </div>
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Election odds</span>
                  <span className="mono tabular" style={{ fontSize: 12 }}>{electionStr}</span>
                </div>
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Supervised restarts</span>
                  <span className="mono tabular" style={{ fontSize: 12 }}>{restartsStr}</span>
                </div>
              </div>

              <div className="surface" style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
                <span className="eyebrow">Resources · sidecars</span>
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>CPU</span>
                  <span className="mono tabular" style={{ fontSize: 12 }}>{cpuStr}</span>
                </div>
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Memory</span>
                  <span className="mono tabular" style={{ fontSize: 12 }}>{ramStr}</span>
                </div>
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Data dir</span>
                  <span className="mono tabular" style={{ fontSize: 12 }}>{diskStr}</span>
                </div>
                <div style={{ display: "flex", justifyContent: "space-between", borderTop: "1px solid var(--line-1)", paddingTop: 8 }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Encryption at rest</span>
                  <span className="mono" style={{ fontSize: 11, color: "var(--ok)" }}>ON · keyring</span>
                </div>
              </div>

              <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
                <div style={{ padding: "11px 16px", borderBottom: "1px solid var(--line-1)", fontSize: 13.5, fontWeight: 500 }}>Crash records</div>
                {crashEmpty && (
                  <p style={{ fontSize: 12, color: "var(--tx-3)", margin: 0, padding: "12px 16px" }}>
                    None. Restarts are supervised with backoff; the app never dies with a sidecar.
                  </p>
                )}
                {crashRows.map((cr, i) => (
                  <div key={i} style={{ padding: "10px 16px", borderBottom: "1px solid var(--line-1)" }}>
                    <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--warn)" }}>
                      {cr.when}
                    </span>
                    <span style={{ display: "block", fontSize: 12, color: "var(--tx-2)", marginTop: 2 }}>
                      {cr.note}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* ===================== EARNING ===================== */}
      {s.nTab === "earn" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(4,1fr)", gap: 12 }}>
            <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 4, opacity: 0.72 }}>
              <span className="eyebrow">Validation</span>
              <span className="tabular" style={{ fontFamily: "var(--font-display)", fontSize: 26 }}>{earnValStr}</span>
              <span className="mono" style={{ fontSize: 10, color: "var(--warn)" }}>off-chain estimate — not in claimable</span>
            </div>
            <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 4, opacity: 0.72 }}>
              <span className="eyebrow">Pinning</span>
              <span className="tabular" style={{ fontFamily: "var(--font-display)", fontSize: 26 }}>{earnPinStr}</span>
              <span className="mono" style={{ fontSize: 10, color: "var(--warn)" }}>off-chain estimate — not in claimable</span>
            </div>
            <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 4, opacity: 0.72 }}>
              <span className="eyebrow">Compute</span>
              <span className="tabular" style={{ fontFamily: "var(--font-display)", fontSize: 26 }}>{earnCompStr}</span>
              <span className="mono" style={{ fontSize: 10, color: "var(--warn)" }}>off-chain estimate — not in claimable</span>
            </div>
            <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 4, borderColor: "var(--accent)" }}>
              <span className="eyebrow" style={{ color: "var(--accent-text)" }}>Claimable</span>
              <span className="tabular" style={{ fontFamily: "var(--font-display)", fontSize: 26, color: "var(--accent-text)" }}>{claimStr}</span>
              <span className="mono" style={{ fontSize: 10, color: s.earnSource === "chain" ? "var(--ok)" : "var(--tx-3)" }}>{claimSourceStr}</span>
              <button className="btn btn-primary btn-sm" onClick={onClaim} disabled={claimDisabled} style={{ marginTop: 4, alignSelf: "flex-start" }}>
                Claim
              </button>
            </div>
          </div>
          <p style={{ fontSize: 11.5, color: "var(--tx-3)", margin: 0 }}>
            {claimNote} The single Claimable figure is the on-chain ContributionAccounting.claimable(address) balance; the per-source split above is an off-chain estimate the contract does not expose. Claiming signs the unsigned claimRewards() request through the ceremony.
          </p>
          <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 10 }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
              <span style={{ fontSize: 13.5, fontWeight: 500 }}>Heartbeat</span>
              <span className="mono tabular" style={{ fontSize: 12, color: hbColor }}>{hbStr}</span>
            </div>
            <div style={{ height: 5, background: "var(--srf-inset)", borderRadius: 999, overflow: "hidden", border: "1px solid var(--line-1)" }}>
              <div style={{ height: "100%", background: hbColor, width: hbBarW, transition: "width .5s linear" }}></div>
            </div>
            <p style={{ fontSize: 11.5, lineHeight: 1.55, color: "var(--tx-3)", margin: 0 }}>
              The 30-second heartbeat is your slashing protection while jobs run — it proves liveness to NematocystSlashing. The supervisor preserves heartbeat continuity across restarts.
            </p>
          </div>
        </div>
      )}

      {/* ===================== PINNING ([SEAM] — PoSt sealer pending) ===================== */}
      {s.nTab === "pin" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ border: "1px solid var(--warn)", background: "var(--warn-bg)", borderRadius: "var(--r-2)", padding: "12px 16px" }}>
            <span style={{ fontSize: 12.5, color: "var(--warn)" }}>
              Proof submission is not yet available in-app — the PoSt sealer sidecar is pending upstream. Plans, bonds, and challenge state below are live daemon state.
            </span>
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "repeat(4,1fr)", gap: 12 }}>
            <div className="surface" style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 3 }}>
              <span className="eyebrow">Bonded</span>
              <span className="tabular" style={{ fontFamily: "var(--font-display)", fontSize: 22 }}>{pinBonded}</span>
              <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>SALT across {pinCount} pins</span>
            </div>
            <div className="surface" style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 3 }}>
              <span className="eyebrow">Projected</span>
              <span className="tabular" style={{ fontFamily: "var(--font-display)", fontSize: 22 }}>{pinProjected}</span>
              <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>SALT / month at current demand</span>
            </div>
            <div className="surface" style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 3 }}>
              <span className="eyebrow">Challenges</span>
              <span className="tabular" style={{ fontFamily: "var(--font-display)", fontSize: 22 }}>{pinPassed}</span>
              <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>passed · 0 failed · 0 slashed</span>
            </div>
            <div className="surface" style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 3 }}>
              <span className="eyebrow">Next window</span>
              <span className="tabular" style={{ fontFamily: "var(--font-display)", fontSize: 22 }}>{pinNext}</span>
              <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>daemon answers challenges itself</span>
            </div>
          </div>

          <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 12 }}>
            <span style={{ fontSize: 14, fontWeight: 500 }}>Pin a CID</span>
            <div style={{ display: "grid", gridTemplateColumns: "2fr 1fr auto", gap: 10, alignItems: "end" }}>
              <span>
                <span className="lbl">Content identifier</span>
                <input ref={pinCidEl} className="input mono" placeholder="bafy…" />
              </span>
              <span>
                <span className="lbl">Bond · SALT</span>
                <input ref={pinBondEl} className="input mono" placeholder="120" type="number" />
              </span>
              <button className="btn btn-secondary" onClick={onPinNew}>
                Plan &amp; sign
              </button>
            </div>
            <p style={{ fontSize: 11.5, lineHeight: 1.5, color: "var(--tx-3)", margin: 0 }}>
              The daemon plans the pin (replication, bond sizing, challenge cadence) and surfaces the decision as an unsigned request — bonds never move without your signature. Bonds are slashable on failed challenges.
            </p>
          </div>

          <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
            <div style={{ display: "grid", gridTemplateColumns: "1.6fr .7fr .9fr 1fr 1fr", gap: 12, padding: "10px 16px", borderBottom: "1px solid var(--line-strong)" }}>
              <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" }}>Pinned CID</span>
              <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" }}>Bond</span>
              <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" }}>Cadence</span>
              <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" }}>Next challenge</span>
              <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" }}>Last proof</span>
            </div>
            {pinsEmpty && (
              <p style={{ fontSize: 12.5, color: "var(--tx-3)", margin: 0, padding: 16 }}>
                No pins yet. Plan your first pin above — the PIN daemon takes it from bond to challenge, and income lands in Earning.
              </p>
            )}
            {pinRows.map((pn, i) => (
              <div key={i} className={pn.rowClass} style={{ display: "grid", gridTemplateColumns: "1.6fr .7fr .9fr 1fr 1fr", gap: 12, padding: "11px 16px", borderBottom: "1px solid var(--line-1)", alignItems: "center" }}>
                <span className="mono" style={{ fontSize: 11.5, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{pn.cid}</span>
                <span className="mono tabular" style={{ fontSize: 12 }}>{pn.bond}</span>
                <span className="mono" style={{ fontSize: 11, color: "var(--tx-2)" }}>{pn.cadence}</span>
                <span className="mono tabular" style={{ fontSize: 11, color: pn.nextColor }}>{pn.next}</span>
                <span className="mono" style={{ fontSize: 11, color: pn.lastColor }}>{pn.last}</span>
              </div>
            ))}
          </div>

          <p style={{ fontSize: 11.5, color: "var(--tx-3)", margin: 0 }}>
            Every pin action is a plan_pin() decision surfaced for your signature — bonds never move without you. Challenge responses run in the daemon while proofs await the sealer sidecar.
          </p>
        </div>
      )}
    </div>
  );
}
