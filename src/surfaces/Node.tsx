import { useEffect } from "react";
import { SurfaceProps } from "./shared";
import { nodeLabel } from "../shell/state";
import { CONSENSUS_LAG_NOTE, producerShortfall, producerWindowLabel, producingWord, readsValidatorRewards, saltFromWei, validatorWord } from "../shell/chainTruth";
import { grantExplanation } from "../data/grantCopy";
import { fmtSaltFromWei } from "../shell/state";
import { BRIDGE_MODE } from "../bridge/mode";

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
  // When the Earning tab is open, read the validator rewards from
  // ValidatorRegistry.rewardsOf(pubkey) (C1-V V8; UI truth contract §2).
  // The producer sample (cached 5 min in the store) behind the "Producing" row.
  useEffect(() => {
    void store.refreshProducers();
  }, [store]);
  // Read again when the reward eligibility changes while the tab is open (the stake or the bond's
  // activated flag reconciles after mount; #348 review).
  const readsRewards = readsValidatorRewards(s.bondedStake, s.bondActivated);
  useEffect(() => {
    if (s.nTab === "earn") void store.refreshEarnings();
  }, [store, s.nTab, readsRewards]);

  // ----- header / tabs -----
  const ntabs: [string, string][] = [
    ["ops", "Operations"],
    ["earn", "Rewards"],
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
  // PRODUCING (C1-V review): "Producing blocks" only when this node's proposer key is among the
  // distinct proposerPubkeys sampled over the last 100 blocks (contract §1, §2; chain-team ruling
  // on the window). Not seen → "Not seen producing in the last 100 blocks", never "not producing".
  // No sample → "—" plus the reason. There is still no per-proposer COUNT, so none is shown.
  const isRegisteredValidator = s.bondedStake > 0;
  // C1-V (contract v1.1 §1): the validator word comes from the registry only (stakeOf, isActive,
  // validatorInfo.actEpoch, currentEpoch). null = registered stake but the registry not read → "—".
  const vWord = validatorWord(s.bondedStake, s.validatorActivation, s.hasGrant, s.bondActivated);
  const pWord = producingWord(s.producers, s.validatorPubkey);
  const producingStr = !isRegisteredValidator
    ? "— · not a registered validator"
    : pWord
      ? pWord
      : s.producers && producerShortfall(s.producers)
        ? "— · producer sample incomplete: " + producerShortfall(s.producers)
        : s.producersError
        ? "— · producer sample unavailable: " + s.producersError
        : !s.validatorPubkey
          ? "— · validator key not read yet"
          : "— · not sampled yet";
  const restartsStr = String(s.crashes.length);

  // ----- earning -----
  // C1-V V8: the three figures are ValidatorRegistry.rewardsOf(pubkey) and nothing else (UI truth
  // contract §1 Rewards): total, claimableNow, and maturing = total − claimableNow. No per-source
  // split and no estimates. Not a validator, or not read → "—" with the reason.
  const rTotal = s.rewards ? saltFromWei(s.rewards.totalWei) : null;
  const rClaim = s.rewards ? saltFromWei(s.rewards.claimableWei) : null;
  const rMaturing = rTotal != null && rClaim != null ? Math.max(0, rTotal - rClaim) : null;
  // An activated bond is read whatever its stake: after a full exit it can still claim (#348 review).
  const rewardsReason = readsValidatorRewards(s.bondedStake, s.bondActivated) ? "not read yet" : "Not a validator";
  const rewardsSrc = "ValidatorRegistry.rewardsOf(pubkey)";
  const claimDisabled = rClaim == null || rClaim < 5;
  const claimNote =
    rClaim == null
      ? "Validator rewards are paid to the block producer only."
      : rClaim < 5
        ? "Claims batch until ≥ 5 SALT to avoid dust gas — " + fmt2(5 - rClaim) + " SALT to go."
        : "Claimable now is above the 5 SALT dust threshold.";
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
                {/* C1-V V5: the one grant-custody explanation (UI truth contract §5.4). */}
                {s.hasGrant && (
                  <p style={{ fontSize: 11.5, color: "var(--tx-3)", margin: 0, lineHeight: 1.5 }}>
                    {grantExplanation({ amountSalt: fmtSaltFromWei(s.s5StakeWei), unlockBlock: s.s5UnlockBlock, staked: s.bondedStake > 0, granted: s.hasGrant })}
                  </p>
                )}
                {/* W1.3 — once the node is synced (proposer.key minted), the member
                    can bond the 32k and become a block producer. staker = the EOA;
                    the tx is approved through the ceremony. Shown only in the
                    packaged app when synced-but-not-yet-validating. A registered stake that
                    isn't active yet (pending, or isActive unread) is already staked: no
                    activation is offered for it (#348 review). */}
                {/* C1-V2: an Exiting or Slashed key is not offered activation. */}
                {TAURI && s.node === "synced" && !(s.bondedStake > 0) && s.bondActivated !== true && vWord !== "Exiting" && vWord !== "Slashed" && (
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
                    ) : (
                      <>
                        <p style={{ fontSize: 12, color: "var(--tx-2)", margin: "0 0 2px", lineHeight: 1.5 }}>
                          Your node is synced and this wallet is linked to your identity. Activating stakes your membership
                          bond in the ValidatorRegistry; the registry sets the epoch your validator becomes active, shown here once registered.
                        </p>
                        <button
                          className="btn btn-primary btn-sm"
                          style={{ alignSelf: "flex-start" }}
                          onClick={() => void store.activateValidator()}
                        >
                          Activate validator · stake your bond
                        </button>
                      </>
                    )}
                  </>
                )}
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Status</span>
                  <span className="mono tabular" style={{ fontSize: 12, color: vWord === "Validator active" ? "var(--ok)" : "var(--tx-3)" }} title="ValidatorRegistry.isActive(pubkey), validatorInfo(pubkey).actEpoch, currentEpoch() and stakeOf(pubkey)">
                    {vWord ?? "— · registry not read yet"}
                    {vWord === "Validator active" && (
                      <span style={{ display: "block", fontSize: 10, color: "var(--tx-3)" }}>{CONSENSUS_LAG_NOTE}</span>
                    )}
                  </span>
                </div>
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Your stake</span>
                  <span className="mono tabular" style={{ fontSize: 12 }} title="ValidatorRegistry.stakeOf(pubkey)">
                    {isRegisteredValidator ? `${fmtI(s.bondedStake)} SALT` : "—"}
                  </span>
                </div>
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Producing</span>
                  <span className="mono tabular" style={{ fontSize: 12, textAlign: "right" }} title={"distinct proposerPubkey in the " + (s.producers ? s.producers.window + " blocks sampled" : "sampled blocks") + " · rpc.citrate.ai"}>
                    {producingStr}
                    {pWord && s.producers && <span style={{ display: "block", fontSize: 10, color: "var(--tx-3)" }}>{producerWindowLabel(s.producers)}</span>}
                  </span>
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
          <div style={{ display: "grid", gridTemplateColumns: "repeat(3,1fr)", gap: 12 }}>
            {(
              [
                ["Validator rewards", rTotal],
                ["Claimable now", rClaim],
                ["Maturing", rMaturing],
              ] as [string, number | null][]
            ).map(([label, v]) => (
              <div key={label} className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 4, borderColor: label === "Claimable now" ? "var(--accent)" : undefined }}>
                <span className="eyebrow" style={label === "Claimable now" ? { color: "var(--accent-text)" } : undefined}>{label}</span>
                <span className="tabular" style={{ fontFamily: "var(--font-display)", fontSize: 26, color: label === "Claimable now" && v != null ? "var(--accent-text)" : undefined }}>
                  {v == null ? "—" : fmt2(v)}
                </span>
                <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>{v == null ? rewardsReason : "SALT · " + rewardsSrc}</span>
                {label === "Claimable now" && (
                  <button className="btn btn-primary btn-sm" onClick={onClaim} disabled={claimDisabled} style={{ marginTop: 4, alignSelf: "flex-start" }}>
                    Claim
                  </button>
                )}
              </div>
            ))}
          </div>
          <p style={{ fontSize: 11.5, color: "var(--tx-3)", margin: 0 }}>
            {claimNote} All three figures are read from {rewardsSrc}: rewards for producing blocks, paid to the producer. Maturing rewards stay slashable for 4 epochs before they become claimable. Claiming signs the unsigned claimRewards() request through the ceremony.
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
