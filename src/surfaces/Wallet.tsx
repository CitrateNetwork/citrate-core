// =====================================================================
// citrate-core — Wallet surface (1:1 from design/CitrateCore.dc.html)
// Sub-tabs: Overview / Staking / Activity / Identity. All mutations go
// through store.setState against existing AppState fields; every signature
// goes through store.requestSig(...) (the ceremony) — no signing invented
// here. Data-source captions are verbatim.
//
// Data source — `liquid` (native eth_getBalance) and `claimable`
// (ContributionAccounting) are REAL 40204 reads via store.refreshWallet(); staked
// (grant-attributed) and activity remain to be grounded (staking-pool view +
// indexer). Wiring replaces the sim, not the UI (Rule 1).
// =====================================================================
import { useRef, useEffect } from "react";
import { SurfaceProps } from "./shared";
import { makeAddr, short, PERSONAS } from "../shell/state";
import { scanTxUrl, scanAddrUrl } from "../data/links";

const fmtI = (n: number) => Math.round(n).toLocaleString("en-US");
const fmt2 = (n: number) => n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
const rel = (ts: number) => {
  const m = (Date.now() - ts) / 60000;
  if (m < 1) return "now";
  if (m < 60) return (m | 0) + "m";
  const h = m / 60;
  if (h < 24) return (h | 0) + "h";
  return ((h / 24) | 0) + "d";
};

const WTABS: [string, string][] = [
  ["overview", "Overview"],
  ["staking", "Staking"],
  ["activity", "Activity"],
  ["identity", "Identity"],
];

export function Wallet({ store, s }: SurfaceProps) {
  const P = PERSONAS[s.persona] || PERSONAS.p1;
  const staked = (s.hasGrant ? 32000 : 0) + s.selfStake;
  const src = s.node === "off" ? "rpc.citrate.ai" : "local node";

  // Fold the REAL liquid (eth_getBalance) + claimable balances on mount, plus the
  // REAL pending-withdrawal queue (WP2). In a Tauri build these read live 40204;
  // in web-dev the sim adapter echoes state / returns an empty queue.
  useEffect(() => {
    void store.refreshWallet();
    void store.refreshPendingWithdrawals();
  }, [store]);

  // input refs (imperative, matching the design's sendToEl/… element refs)
  const sendToEl = useRef<HTMLInputElement | null>(null);
  const sendAmtEl = useRef<HTMLInputElement | null>(null);
  const stakeAmtEl = useRef<HTMLInputElement | null>(null);
  const unstakeAmtEl = useRef<HTMLInputElement | null>(null);

  const sponsTxt =
    s.sponsorUnits > 0 ? "gas sponsored — " + s.sponsorUnits + " of 5 daily units left" : "you pay gas — daily sponsorship budget exhausted";
  const sponsorLine = s.sponsorUnits + " of 5 units · resets 00:00 UTC · category standard";
  const sponsorBarW = (s.sponsorUnits / 5) * 100 + "%";
  const sponsorBarColor = s.sponsorUnits > 1 ? "var(--accent)" : "var(--warn)";

  const saltStr = fmt2(s.liquid);
  const stakedStr = staked ? fmtI(staked) : "0";
  const wsaltStr = "0.00";
  const stakedSub = s.hasGrant
    ? "32,000 grant vaulted" + (s.selfStake ? " + " + fmtI(s.selfStake) + " self" : "")
    : "no grant — join to stake";
  const rewardsStr = fmt2(s.earnVal + s.earnPin + s.earnComp);
  const grantStr = s.hasGrant ? "32,000 SALT" : "—";
  const selfStakeStr = fmtI(s.selfStake) + " SALT";

  const onCopyAddr = () => store.copy(s.walletAddr, "Address copied");

  // Send is a REAL native SALT transfer: build the pending ceremony + broadcast
  // the real 40204 tx (store.walletSend → bridge.wallet.send + signing.broadcast).
  // No local balance mutation, no fabricated hash — the balance re-reads from
  // chain after settle. Amount is converted to wei (18 decimals) for the command.
  const onSendTx = () => {
    const to = sendToEl.current ? sendToEl.current.value.trim() : "";
    const amtRaw = sendAmtEl.current ? sendAmtEl.current.value.trim() : "";
    if (!/^0x[0-9a-fA-F]{40}$/.test(to)) return store.toast("Enter a valid destination address (0x + 40 hex)");
    // STRICT plain-decimal only — must match the BigInt wei path exactly. Rejects
    // scientific ("1e3"), separators ("1,000"), multi-dot ("1.2.3"), "Infinity",
    // etc. (which pass parseFloat but throw in BigInt → a silent no-op).
    if (!/^(\d+\.?\d*|\.\d+)$/.test(amtRaw)) return store.toast("Enter a valid amount (e.g. 1.5)");
    const amt = parseFloat(amtRaw);
    if (!(amt > 0)) return store.toast("Enter an amount greater than zero");
    if (amt > s.liquid) return store.toast("Exceeds your liquid balance");
    // Decimal SALT → wei (18 dp) via BigInt (no float error). Excess precision is
    // TRUNCATED (never rounded up) so we can never send more than typed.
    let wei: string;
    try {
      const [whole, frac = ""] = amtRaw.split(".");
      wei = (BigInt(whole || "0") * 10n ** 18n + BigInt((frac + "0".repeat(18)).slice(0, 18))).toString();
    } catch {
      return store.toast("Enter a valid amount (e.g. 1.5)");
    }
    if (wei === "0") return store.toast("Enter an amount greater than zero");
    void store.walletSend(to, wei).then(() => {
      if (sendToEl.current) sendToEl.current.value = "";
      if (sendAmtEl.current) sendAmtEl.current.value = "";
    });
  };

  // Add stake is a REAL LiquidStakingPool deposit(): build the pending ceremony +
  // broadcast the real 40204 tx (store.walletStake → bridge.wallet.stake +
  // signing.broadcast). No local balance mutation, no fabricated hash — the staked
  // figure re-reads from chain after settle. Amount is SALT → wei (18 dp) via the
  // same STRICT decimal path as Send (never sends more than typed).
  const onAddStake = () => {
    const amtRaw = stakeAmtEl.current ? stakeAmtEl.current.value.trim() : "";
    if (!/^(\d+\.?\d*|\.\d+)$/.test(amtRaw)) return store.toast("Enter a valid amount (e.g. 1.5)");
    const amt = parseFloat(amtRaw);
    if (!(amt > 0)) return store.toast("Enter an amount greater than zero");
    if (amt > s.liquid) return store.toast("Exceeds your liquid balance");
    let wei: string;
    try {
      const [whole, frac = ""] = amtRaw.split(".");
      wei = (BigInt(whole || "0") * 10n ** 18n + BigInt((frac + "0".repeat(18)).slice(0, 18))).toString();
    } catch {
      return store.toast("Enter a valid amount (e.g. 1.5)");
    }
    if (wei === "0") return store.toast("Enter an amount greater than zero");
    void store.walletStake(wei).then(() => {
      if (stakeAmtEl.current) stakeAmtEl.current.value = "";
    });
  };

  // Withdraw is a REAL LiquidStakingPool requestWithdrawal (WP2): the SALT amount
  // is converted to shares IN RUST from live reads, then the pending ceremony is
  // built + broadcast (store.walletRequestWithdrawal → bridge.wallet
  // .requestWithdrawal + signing.broadcast). It burns shares into the ~7-day queue
  // (claim later via the Pending panel). No local balance mutation, no fabricated
  // hash. Amount is SALT → wei (18 dp) via the SAME STRICT decimal path as Send.
  const onUnstake = () => {
    const amtRaw = unstakeAmtEl.current ? unstakeAmtEl.current.value.trim() : "";
    if (!/^(\d+\.?\d*|\.\d+)$/.test(amtRaw)) return store.toast("Enter a valid amount (e.g. 1.5)");
    const amt = parseFloat(amtRaw);
    if (!(amt > 0)) return store.toast("Enter an amount greater than zero");
    if (amt > s.selfStake) return store.toast("Only self-added stake can be withdrawn — granted principal is vaulted");
    let wei: string;
    try {
      const [whole, frac = ""] = amtRaw.split(".");
      wei = (BigInt(whole || "0") * 10n ** 18n + BigInt((frac + "0".repeat(18)).slice(0, 18))).toString();
    } catch {
      return store.toast("Enter a valid amount (e.g. 1.5)");
    }
    if (wei === "0") return store.toast("Enter an amount greater than zero");
    void store.walletRequestWithdrawal(wei).then(() => {
      if (unstakeAmtEl.current) unstakeAmtEl.current.value = "";
    });
  };

  // WP2 — the REAL pending-withdrawal queue (chain-sourced via
  // store.refreshPendingWithdrawals). Each row is claimable only once the ~7-day
  // (50,400-block) delay has elapsed on-chain; Claim → store.walletClaimWithdrawal.
  const pending = s.pendingWithdrawals;
  const onClaimWithdrawal = (id: string) => void store.walletClaimWithdrawal(id);

  const activityRows = s.activity.map((a, i) => ({
    kind: a.kind,
    hash: a.hash,
    hashShort: short(a.hash),
    time: rel(a.ts),
    amount: a.amount,
    amtColor: a.amount && a.amount.indexOf("−") === 0 ? "var(--tx-1)" : "var(--accent-text)",
    rowClass: i === 0 && s.justSigned ? "cc-row-stroke" : "",
  }));
  const txEmpty = activityRows.length === 0;

  // Primary is the REAL claim wallet. The extra linked wallet + agent SBT are
  // sim-persona cosmetics only — never shown to a real signed-in user (Rule 1).
  const linkedWallets = [{ addr: short(s.walletAddr), label: "smart wallet · primary" }].concat(
    !s.signedIn && s.persona !== "p1" ? [{ addr: short(makeAddr(P.name + "x")), label: "linked · SIWE proof" }] : [],
  );
  const agents = !s.signedIn && s.persona === "p3" ? [{ name: "research-runner", id: "AgentSBT #221 · parent #4187" }] : [];
  const agentsEmpty = agents.length === 0;

  const uc = { fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase" as const, color: "var(--tx-3)" };

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Wallet</span>
        <span style={{ display: "flex", gap: 2, background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: 2 }}>
          {WTABS.map(([id, label]) => (
            <button
              key={id}
              onClick={() => {
                store.setState({ wTab: id });
                store.save();
              }}
              style={{
                fontFamily: "var(--font-sans)",
                fontSize: 12,
                fontWeight: s.wTab === id ? 500 : 400,
                padding: "5px 12px",
                border: "none",
                borderRadius: 5,
                cursor: "pointer",
                background: s.wTab === id ? "var(--srf-2)" : "transparent",
                color: s.wTab === id ? "var(--tx-1)" : "var(--tx-3)",
              }}
            >
              {label}
            </button>
          ))}
        </span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 10, letterSpacing: ".08em", color: "var(--tx-3)" }}>
          source · {src}
        </span>
      </div>

      {/* ---------------- Overview ---------------- */}
      {s.wTab === "overview" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(3,1fr)", gap: 12 }}>
            <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 4 }}>
              <span className="eyebrow">SALT · liquid</span>
              <span className="tabular" style={{ fontFamily: "var(--font-display)", fontWeight: 400, fontSize: 30 }}>
                {saltStr}
              </span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                earned rewards — spendable
              </span>
            </div>
            <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 4 }}>
              <span className="eyebrow">Staked</span>
              <span className="tabular" style={{ fontFamily: "var(--font-display)", fontWeight: 400, fontSize: 30 }}>
                {stakedStr}
              </span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                {stakedSub}
              </span>
            </div>
            <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 4 }}>
              <span className="eyebrow">wSALT</span>
              <span className="tabular" style={{ fontFamily: "var(--font-display)", fontWeight: 400, fontSize: 30 }}>
                {wsaltStr}
              </span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                wrapped — none held
              </span>
            </div>
          </div>

          <div className="surface" style={{ padding: "12px 18px", display: "flex", alignItems: "center", gap: 16 }}>
            <span className="eyebrow" style={{ whiteSpace: "nowrap" }}>
              Paymaster
            </span>
            <div style={{ flex: 1, height: 5, background: "var(--srf-inset)", borderRadius: 999, overflow: "hidden", border: "1px solid var(--line-1)" }}>
              <div style={{ height: "100%", background: sponsorBarColor, width: sponsorBarW, transition: "width var(--dur-base) var(--ease-standard)" }}></div>
            </div>
            <span className="mono tabular" style={{ fontSize: 11, color: "var(--tx-2)", whiteSpace: "nowrap" }}>
              {sponsorLine}
            </span>
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
              <span style={{ fontSize: 14, fontWeight: 500 }}>Send</span>
              <span>
                <span className="lbl">To address</span>
                <input ref={sendToEl} className="input mono" placeholder="0x…" />
              </span>
              <span>
                <span className="lbl">Amount · SALT</span>
                <input ref={sendAmtEl} className="input mono" placeholder="0.00" type="number" />
              </span>
              <span style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <button className="btn btn-secondary" onClick={onSendTx}>
                  Review &amp; sign
                </button>
                <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>
                  {sponsTxt}
                </span>
              </span>
            </div>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
              <span style={{ fontSize: 14, fontWeight: 500 }}>Receive</span>
              <span className="lbl">Your smart wallet · chain 40204</span>
              <span className="mono" style={{ fontSize: 12.5, wordBreak: "break-all", background: "var(--srf-inset)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: "10px 12px" }}>
                {s.walletAddr}
              </span>
              <span style={{ display: "flex", gap: 8 }}>
                <button className="btn btn-ghost btn-sm" onClick={onCopyAddr}>
                  Copy address
                </button>
              </span>
              <p style={{ fontSize: 11.5, lineHeight: 1.5, color: "var(--tx-3)", margin: 0 }}>
                Deploys lazily on first outgoing action. Deposits to this address are safe now.
              </p>
            </div>
          </div>
        </div>
      )}

      {/* ---------------- Staking ---------------- */}
      {s.wTab === "staking" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 14 }}>
            <div style={{ display: "flex", alignItems: "baseline", gap: 14 }}>
              <span className="tabular" style={{ fontFamily: "var(--font-display)", fontWeight: 400, fontSize: 34 }}>
                {stakedStr}
              </span>
              <span className="eyebrow" style={{ color: "var(--accent-text)" }}>
                SALT staked
              </span>
              <span className="mono tabular" style={{ marginLeft: "auto", fontSize: 12, color: "var(--tx-2)" }}>
                rewards accrued · {rewardsStr} SALT
              </span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 8, borderTop: "1px solid var(--line-1)", paddingTop: 12 }}>
              <div style={{ display: "flex", justifyContent: "space-between" }}>
                <span style={{ fontSize: 13, color: "var(--tx-2)" }}>Membership grant · vaulted</span>
                <span className="mono tabular" style={{ fontSize: 13 }}>
                  {grantStr}
                </span>
              </div>
              <div style={{ display: "flex", justifyContent: "space-between" }}>
                <span style={{ fontSize: 13, color: "var(--tx-2)" }}>Self-added stake</span>
                <span className="mono tabular" style={{ fontSize: 13 }}>
                  {selfStakeStr}
                </span>
              </div>
              <div style={{ display: "flex", justifyContent: "space-between" }}>
                <span style={{ fontSize: 13, color: "var(--tx-2)" }}>Validator minimum</span>
                <span className="mono tabular" style={{ fontSize: 13 }}>
                  32,000 SALT
                </span>
              </div>
            </div>
            <p style={{ fontSize: 11.5, lineHeight: 1.55, color: "var(--tx-3)", margin: 0 }}>
              Granted principal is vaulted until mainnet release and cannot be withdrawn — by construction, not by UI. Validator reward eligibility ends below 32,000 SALT staked.
            </p>
          </div>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
              <span style={{ fontSize: 14, fontWeight: 500 }}>Add stake</span>
              <span>
                <span className="lbl">Amount · from liquid balance</span>
                <input ref={stakeAmtEl} className="input mono" placeholder="0.00" type="number" />
              </span>
              <span>
                <button className="btn btn-secondary" onClick={onAddStake}>
                  Review &amp; sign
                </button>
              </span>
            </div>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
              <span style={{ fontSize: 14, fontWeight: 500 }}>Withdraw self-added stake</span>
              <span>
                <span className="lbl">Amount · 7-day lockup applies</span>
                <input ref={unstakeAmtEl} className="input mono" placeholder="0.00" type="number" />
              </span>
              <span>
                <button className="btn btn-ghost" onClick={onUnstake}>
                  Review &amp; sign
                </button>
              </span>
              <p style={{ fontSize: 11, lineHeight: 1.5, color: "var(--tx-3)", margin: 0 }}>
                Requesting burns your stSALT shares into a queue. The SALT unlocks after ~7 days (50,400 blocks), then you Claim it below. No instant settlement.
              </p>
            </div>
          </div>

          {/* ------- Pending withdrawals (WP2, real on-chain queue) ------- */}
          <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
            <div style={{ padding: "12px 16px", borderBottom: "1px solid var(--line-1)", display: "flex", alignItems: "center", gap: 10 }}>
              <span style={{ fontSize: 13.5, fontWeight: 500 }}>Pending withdrawals</span>
              <span className="mono" style={{ marginLeft: "auto", fontSize: 10, letterSpacing: ".08em", color: "var(--tx-3)" }}>
                source · on-chain queue
              </span>
            </div>
            {pending.length === 0 && (
              <p style={{ fontSize: 12.5, color: "var(--tx-3)", margin: 0, padding: 16 }}>
                No pending withdrawals. A requested withdrawal appears here and becomes claimable after the ~7-day (50,400-block) delay.
              </p>
            )}
            {pending.map((w) => {
              const saltStr = fmt2(Number(BigInt(w.saltWei)) / 1e18);
              const blocksLeft = Math.max(0, w.claimableAtBlock - s.height);
              const daysLeft = (blocksLeft * 12) / 86400; // ~12s blocks
              const statusTxt = w.claimable ? "claimable now" : "~" + fmtI(blocksLeft) + " blocks (~" + daysLeft.toFixed(1) + "d) left";
              return (
                <div key={w.id} style={{ display: "grid", gridTemplateColumns: "1fr 1.2fr auto", gap: 12, padding: "11px 16px", borderBottom: "1px solid var(--line-1)", alignItems: "center" }}>
                  <span className="mono tabular" style={{ fontSize: 12.5 }}>
                    {saltStr} SALT
                  </span>
                  <span className="mono" style={{ fontSize: 11, color: w.claimable ? "var(--accent-text)" : "var(--tx-3)" }}>
                    {statusTxt}
                  </span>
                  <button className="btn btn-secondary btn-sm" disabled={!w.claimable} onClick={() => onClaimWithdrawal(w.id)}>
                    Claim
                  </button>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* ---------------- Activity ---------------- */}
      {s.wTab === "activity" && (
        <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
          <div style={{ display: "grid", gridTemplateColumns: "1.4fr 1fr .8fr .8fr", gap: 12, padding: "10px 16px", borderBottom: "1px solid var(--line-strong)" }}>
            <span className="mono" style={uc}>
              Action
            </span>
            <span className="mono" style={uc}>
              Transaction
            </span>
            <span className="mono" style={uc}>
              Amount
            </span>
            <span className="mono" style={{ ...uc, textAlign: "right" }}>
              When
            </span>
          </div>
          {txEmpty && (
            <p style={{ fontSize: 12.5, color: "var(--tx-3)", margin: 0, padding: 16 }}>
              No signed actions yet. This is a client-side ledger of your signatures — per-transaction chain detail links out to CitrateScan.
            </p>
          )}
          {activityRows.map((tx, i) => (
            <div key={i} className={tx.rowClass} style={{ display: "grid", gridTemplateColumns: "1.4fr 1fr .8fr .8fr", gap: 12, padding: "11px 16px", borderBottom: "1px solid var(--line-1)", alignItems: "center" }}>
              <span style={{ fontSize: 12.5, fontWeight: 500 }}>{tx.kind}</span>
              <button
                className="mono"
                onClick={() => void store.openExternal(scanTxUrl(tx.hash))}
                title="Open on CitrateScan"
                style={{ fontSize: 11, color: "var(--accent-text)", cursor: "pointer", background: "none", border: "none", padding: 0, textAlign: "left" }}
              >
                {tx.hashShort} ↗
              </button>
              <span className="mono tabular" style={{ fontSize: 12, color: tx.amtColor }}>
                {tx.amount}
              </span>
              <span className="mono tabular" style={{ fontSize: 11, color: "var(--tx-3)", textAlign: "right" }}>
                {tx.time}
              </span>
            </div>
          ))}
        </div>
      )}

      {/* ---------------- Identity ---------------- */}
      {s.wTab === "identity" && (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          {s.hasSbt ? (
            <div className="surface" style={{ padding: 18, display: "flex", gap: 18, alignItems: "center" }}>
              <img src="assets/citrate_mark_green.svg" alt="" style={{ width: 44, height: 44, flexShrink: 0 }} />
              <span style={{ flex: 1, minWidth: 0 }}>
                <span style={{ display: "block", fontSize: 15, fontWeight: 500 }}>CitrateMemberSBT #4187</span>
                <span className="mono" style={{ display: "block", fontSize: 11, color: "var(--tx-3)", marginTop: 2 }}>
                  non-transferable · minted 2026-07-11 · bound to your sub-hash
                </span>
              </span>
              <button
                className="mono"
                onClick={() => void store.openExternal(scanAddrUrl(s.walletAddr))}
                title="Open your address on CitrateScan"
                style={{ fontSize: 11, color: "var(--accent-text)", cursor: "pointer", background: "none", border: "none", padding: 0 }}
              >
                chain proof ↗
              </button>
            </div>
          ) : (
            <div className="surface" style={{ padding: 18 }}>
              <p style={{ fontSize: 13, color: "var(--tx-3)", margin: 0 }}>No membership SBT — it mints with a paid membership at the grant ceremony.</p>
            </div>
          )}

          <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
            <div style={{ padding: "12px 16px", borderBottom: "1px solid var(--line-1)", fontSize: 13.5, fontWeight: 500 }}>Linked wallets</div>
            {linkedWallets.map((lw, i) => (
              <div key={i} style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 16px", borderBottom: "1px solid var(--line-1)" }}>
                <span className="mono" style={{ fontSize: 12, flex: 1 }}>
                  {lw.addr}
                </span>
                <span className="mono" style={{ fontSize: 10, letterSpacing: ".08em", textTransform: "uppercase", color: "var(--tx-3)" }}>
                  {lw.label}
                </span>
              </div>
            ))}
            <div style={{ padding: "10px 16px" }}>
              <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                Link another wallet with a SIWE proof — identity registry list / link / unlink.
              </span>
            </div>
          </div>

          <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
            <div style={{ padding: "12px 16px", borderBottom: "1px solid var(--line-1)", fontSize: 13.5, fontWeight: 500 }}>Registered agents</div>
            {agentsEmpty && (
              <p style={{ fontSize: 12.5, color: "var(--tx-3)", margin: 0, padding: "14px 16px" }}>
                No agents registered. AgentSBTs parent to your member SBT — the on-chain primitive others can build on.
              </p>
            )}
            {agents.map((ag, i) => (
              <div key={i} style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 16px", borderBottom: "1px solid var(--line-1)" }}>
                <span style={{ fontSize: 12.5, fontWeight: 500, flex: 1 }}>{ag.name}</span>
                <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                  {ag.id}
                </span>
              </div>
            ))}
          </div>

          <div style={{ border: "1px dashed var(--line-2)", borderRadius: "var(--r-2)", padding: "16px 18px", display: "flex", flexDirection: "column", gap: 6 }}>
            <span className="eyebrow">Verifiable identity proofs</span>
            <p style={{ fontSize: 12.5, lineHeight: 1.55, color: "var(--tx-3)", margin: 0 }}>
              ZK identity checkpoints are in development — the verify precompile is live on 40204; the identity-checkpoint circuit is not. Nothing to click yet; this page will say so until it&apos;s real.
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
