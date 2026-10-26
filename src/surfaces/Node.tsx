import { useEffect } from "react";
import { SurfaceProps } from "./shared";
import { nodeLabel } from "../shell/state";
import { BRIDGE_MODE } from "../bridge/mode";
import { ActivationTxPanel, hasActivationTxStatus } from "../onboarding/ActivationTxPanel";

// Strip ANSI colour/style escape sequences from a node log line. The node emits
// `tracing` output; even with NO_COLOR set in the spawn env (node.rs) we strip
// defensively here so a stray escape never renders as an unfilled box glyph.
// eslint-disable-next-line no-control-regex
const ANSI_RE = /\x1b\[[0-9;]*m/g;
function stripAnsi(s: string): string {
  return s.replace(ANSI_RE, "");
}

// Q-A.4b — in the PACKAGED (tauri) build there is no real read for these vitals
// yet, so they must render an honest state instead of a fabricated number/row:
//   - blocksProposed: no validator registration exists yet (Q-C) → honest label;
//   - cpu / ram: no trivial process read wired → "—" (never the sim number);
//   - peer ROWS: no per-peer detail until citrate_getDagStats (Q-B.3) → show the
//     REAL peer COUNT + an honest empty state, never fabricated peer rows;
//   - syncPct: a binary 0/100 stub (node.rs) → present "syncing…"/"synced", never
//     a precise fake percent.
// The web-dev/sim path keeps the labelled prototype vitals (they animate honestly
// as a preview and are never shown to a packaged member).
const TAURI = BRIDGE_MODE === "tauri";

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
  stalled: "#e8843c",
  error: "#dd7259",
};

/**
 * Node surface — 1:1 from design/CitrateCore.dc.html NODE section.
 * Operations / Earning / Pinning sub-tabs. Every real datum is captioned with
 * its source (node-agent 127.0.0.1:19600). Claims and pins are unsigned
 * node-agent requests surfaced through the SignatureCeremony (store.requestSig);
 * no key ever leaves the ceremony. Pin bonds (IPFSIncentivesV3) are deployed but
 * dormant on 40204, so the Pinning tab says "Pin bonds not live yet" (UI truth
 * contract §1, §4) instead of drawing bonds, projections or challenge tallies.
 */
export function Node({ store, s }: SurfaceProps) {
  // CORE-C2 — when the Earning tab is open, pull the REAL claimable from
  // ContributionAccounting.claimable(vaultAddress) via eth_call (Rule 11). The
  // decomposition (Validation/Pinning/Compute) has NO on-chain source and is
  // labeled as an off-chain estimate below (Rule 1 / I-3).
  useEffect(() => {
    if (s.nTab === "earn") void store.refreshEarnings();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [s.nTab]);
  // #406 — read the activation (and the durable record of a sent one) on mount, so a recorded
  // activation shows here with its way out (Try again / Forget), as on S7.
  useEffect(() => {
    if (TAURI && s.hasGrant) void store.refreshActivation();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [s.hasGrant]);

  const staked = s.hasGrant ? 32000 : 0;

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
  const noStop = s.node === "off" || s.node === "prov";
  const onNodeStart = () => store.startNode();
  // Q-A.4b item 4 — Pause/Resume were COSMETIC (setState only; no supervisor
  // pause call exists in the bridge). They painted a "paused" label without
  // pausing the real node, so they are removed in favour of the real Stop
  // (bridge.node.stop). If a real supervisor pause lands (Q-C), re-add them wired.
  const onNodeStop = () => store.stopNode();
  const showSync = s.node === "syncing";
  // syncPct is now a REAL percentage: local head vs the authoritative network tip
  // (node.rs, same comparison that gates block production), cached 15s. It used to
  // be a 0/100 stub, which is why this showed a bare "syncing…" and a full-width
  // bar. A negative value means the tip could not be read — render "—" rather than
  // guess (Rule 1); never claim 100 on an unknown.
  const syncKnown = s.syncPct >= 0;
  const syncPctStr = syncKnown ? (s.syncPct | 0) + "%" : "—";
  const syncBarW = syncKnown ? (s.syncPct | 0) + "%" : "0%";
  const logsEmpty = s.node === "off";
  const logLines = s.logs;
  // Q-A.4b item 3 — per-peer rows are DEAD in tauri (no getDagStats yet). Never
  // fabricate peer rows in the packaged build: render the REAL peer count + an
  // honest "peer detail coming" empty state instead. Sim keeps its preview rows.
  const peerRows = TAURI || s.node === "off" ? [] : s.peerRows;
  const peersEmpty = s.node === "off" || peerRows.length === 0;
  // Q-A.4b item 1 — cpu/ram have no real process read wired in the packaged build,
  // and blocksProposed has no validator registration yet (Q-C). Show honest states
  // in tauri, never the fabricated sim number.
  const cpuStr = TAURI ? "—" : s.node === "off" ? "—" : (s.cpu | 0) + " %";
  const ramStr = TAURI ? "—" : s.node === "off" ? "—" : (s.ram | 0) + " MB";
  const diskStr = TAURI ? "—" : s.node === "off" ? "—" : "3.1 GB";
  const crashEmpty = s.crashes.length === 0;
  const crashRows = s.crashes;
  // REGISTRATION is a real, readable fact: the bond clone is the registry staker
  // once `pubkeyOfStaker(bond) != 0`, which the store folds as `bondedStake` from
  // grantStatus. This line used to be the literal "not a registered validator" for
  // EVERY tauri build regardless of truth, so a member who had just bonded 32,000
  // SALT was told they were not a validator (observed 2026-08-06, right after a
  // successful MemberBond.activate).
  //
  // BLOCKS PROPOSED still has NO source — no RPC exposes a per-proposer count — so
  // a registered validator honestly reads "—" rather than a fabricated tally. What
  // changes is that we no longer DENY the registration we can actually see.
  const isRegisteredValidator = s.bondedStake >= 32000;
  const blocksProposedStr = TAURI
    ? isRegisteredValidator
      ? "—"
      : "not a registered validator"
    : s.node === "off"
      ? "—"
      : fmtI(s.blocksProposed);
  const electionStr =
    !TAURI && s.node === "validating" && staked >= 32000 ? ((staked / 8200000) * 100).toFixed(2) + "% stake share / round" : "—";
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
  // C1-V V2: there is no heartbeat source. The node does not report one to the app and no
  // slashing contract reads one, so the panel shows the unavailable state, never a seeded age.

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

      {/* SCL-S0.6 — the node start was refused: another process holds the chain database or a
          node port. The message (from node.rs) names the holder (pid, path) and what to do. */}
      {s.nodeBlocked && (
        <div
          role="alert"
          className="surface"
          style={{ padding: "12px 16px", borderLeft: "3px solid " + nodeColors.error, display: "flex", flexDirection: "column", gap: 4 }}
        >
          <span style={{ fontSize: 13.5, fontWeight: 500 }}>The node cannot start yet</span>
          <span style={{ fontSize: 12.5, color: "var(--tx-2)", overflowWrap: "anywhere" }}>{s.nodeBlocked}</span>
        </div>
      )}

      {/* The mining gate (node.rs) is holding block production; the reason says why. */}
      {s.miningHold && (
        <div
          role="status"
          className="surface"
          style={{ padding: "12px 16px", borderLeft: "3px solid var(--tx-3)", display: "flex", flexDirection: "column", gap: 4 }}
        >
          <span style={{ fontSize: 12.5, color: "var(--tx-2)", overflowWrap: "anywhere" }}>{s.miningHold}</span>
        </div>
      )}

      {/* ===================== OPERATIONS ===================== */}
      {s.nTab === "ops" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div className="surface" style={{ padding: "16px 18px", display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
            <button className="btn btn-primary" onClick={onNodeStart} disabled={noStart}>
              Start
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
            {/* log tail — minWidth:0 lets this flex child shrink instead of the
                long log lines widening the whole column (the "too wide" bug);
                lines wrap within the bounded panel and it scrolls vertically. */}
            <div className="surface" style={{ display: "flex", flexDirection: "column", minHeight: 0, minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "center", padding: "11px 16px", borderBottom: "1px solid var(--line-1)" }}>
                <span style={{ fontSize: 13.5, fontWeight: 500 }}>Log tail</span>
                <span className="mono" style={{ marginLeft: "auto", fontSize: 10, color: "var(--tx-3)" }}>
                  ~/.citrate/core/node.log
                </span>
              </div>
              <div style={{ padding: "10px 16px", display: "flex", flexDirection: "column", gap: 4, maxHeight: 280, overflowY: "auto", overflowX: "hidden", minWidth: 0, background: "var(--srf-inset)" }}>
                {logsEmpty && (
                  <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                    — node is off; no log stream —
                  </span>
                )}
                {logLines.map((lg) => (
                  <span key={lg.id} className="mono tabular" style={{ fontSize: 10.5, lineHeight: 1.6, color: "var(--tx-2)", whiteSpace: "pre-wrap", wordBreak: "break-word", overflowWrap: "anywhere" }}>
                    <span style={{ color: "var(--tx-3)" }}>{lg.t}</span>  {stripAnsi(lg.line)}
                  </span>
                ))}
              </div>
            </div>

            {/* right column */}
            <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
                <div style={{ padding: "11px 16px", borderBottom: "1px solid var(--line-1)", display: "flex", alignItems: "center", gap: 8 }}>
                  <span style={{ fontSize: 13.5, fontWeight: 500 }}>Peers</span>
                  {/* The peer COUNT is a REAL read (net_peerCount). Per-peer detail
                      (id/direction/latency) needs citrate_getDagStats (Q-B.3), so
                      in the packaged build we show the count + an honest empty
                      state rather than fabricated peer rows (Rule 1). */}
                  <span className="mono tabular" style={{ marginLeft: "auto", fontSize: 12, color: "var(--tx-2)" }}>
                    {s.node === "off" ? "—" : s.peers + " connected"}
                  </span>
                </div>
                {peersEmpty && (
                  <p style={{ fontSize: 11.5, color: "var(--tx-3)", margin: 0, padding: "12px 16px" }}>
                    {s.node === "off"
                      ? "—"
                      : TAURI
                        ? "Peer detail coming — per-peer id/latency needs citrate_getDagStats. The connected count above is a real read."
                        : "—"}
                  </p>
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
                {/* W1.3 — once the node is synced (proposer.key minted), the member
                    can bond the 32k and become a block producer. staker = the EOA;
                    the tx is approved through the ceremony. Shown only in the
                    packaged app when synced-but-not-yet-validating. */}
                {TAURI && s.node === "synced" && (
                  <>
                    {/* The bond is paid to the address the AUTHORITY serves as
                        `wallet_address`, but the self-bond is sent from THIS
                        device's custody wallet. Until they are the same address,
                        activating funds somewhere the member cannot spend from —
                        so linking is a prerequisite, not a suggestion. */}
                    {!store.walletIsLinked() ? (
                      <>
                        <p style={{ fontSize: 12, color: "var(--tx-2)", margin: "0 0 2px", lineHeight: 1.5 }}>
                          Your node is synced. Before activating, link this device&rsquo;s wallet to your Citrate
                          identity — your membership bond is paid to the wallet your identity names, and it must be
                          this one or the funds land somewhere this device cannot spend from.
                        </p>
                        <button
                          className="btn btn-primary btn-sm"
                          style={{ alignSelf: "flex-start" }}
                          onClick={() => void store.linkWallet()}
                        >
                          Link this wallet · no funds move
                        </button>
                        <span style={{ fontSize: 11.5, color: "var(--tx-2)" }}>
                          You&rsquo;ll approve a signature that proves you control this wallet. Nothing is spent.
                        </span>
                      </>
                    ) : hasActivationTxStatus(store, s) ? (
                      <ActivationTxPanel store={store} s={s} testId="node-activation" />
                    ) : (
                      <>
                        <p style={{ fontSize: 12, color: "var(--tx-2)", margin: "0 0 2px", lineHeight: 1.5 }}>
                          Your node is synced and this wallet is linked to your identity. Activate it as a validator to
                          bond your 32,000 SALT and start producing blocks + earning.
                        </p>
                        <button
                          className="btn btn-primary btn-sm"
                          style={{ alignSelf: "flex-start" }}
                          onClick={() => void store.activateValidator()}
                        >
                          Activate validator · bond 32,000 SALT
                        </button>
                      </>
                    )}
                  </>
                )}
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Registered</span>
                  <span className="mono tabular" style={{ fontSize: 12, color: isRegisteredValidator ? "var(--ok)" : "var(--tx-3)" }}>
                    {isRegisteredValidator ? `yes · ${fmtI(s.bondedStake)} SALT bonded` : "no"}
                  </span>
                </div>
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
              <span className="mono tabular" style={{ fontSize: 12, color: "var(--tx-3)" }}>—</span>
            </div>
            <p style={{ fontSize: 11.5, lineHeight: 1.55, color: "var(--tx-3)", margin: 0 }}>
              no heartbeat reading: the node does not report a heartbeat to this app yet.
            </p>
          </div>
        </div>
      )}

      {/* ===================== PINNING (dormant on 40204) ===================== */}
      {s.nTab === "pin" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 8 }}>
            <span style={{ fontSize: 14, fontWeight: 500 }}>Pin bonds not live yet</span>
            <p style={{ fontSize: 12.5, lineHeight: 1.55, color: "var(--tx-2)", margin: 0 }}>
              The pin bond contract (IPFSIncentivesV3) is deployed on betanet but not funded, and no pins exist. Bonds, challenges and pin rewards appear here
              once pin bonds go live. Until then nothing here is bonded.
            </p>
            <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>
              source · IPFSIncentivesV3 · address book 40204
            </span>
          </div>
        </div>
      )}
    </div>
  );
}
