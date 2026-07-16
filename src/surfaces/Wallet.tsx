// =====================================================================
// citrate-core — Wallet surface (1:1 from design/CitrateCore.dc.html)
// Sub-tabs: Overview / Staking / Activity / Identity. All mutations go
// through store.setState against existing AppState fields; every signature
// goes through store.requestSig(...) (the ceremony) — no signing invented
// here. Data-source captions are verbatim.
//
// Data source — balances/stake/activity are prototype sim state (freshState);
// the "source · rpc.citrate.ai / local node" caption names where the real
// read lands. Wiring replaces the sim, not the UI (Rule 1).
// =====================================================================
import { useRef } from "react";
import { SurfaceProps } from "./shared";
import { makeAddr, short, PERSONAS } from "../shell/state";

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

  // input refs (imperative, matching the design's sendToEl/… element refs)
  const sendToEl = useRef<HTMLInputElement | null>(null);
  const sendAmtEl = useRef<HTMLInputElement | null>(null);
  const stakeAmtEl = useRef<HTMLInputElement | null>(null);
  const unstakeAmtEl = useRef<HTMLInputElement | null>(null);

  const sponsTxt =
    s.sponsorUnits > 0 ? "gas sponsored — " + s.sponsorUnits + " of 5 daily units left" : "you pay gas — daily sponsorship budget exhausted";
  const sponsColor = s.sponsorUnits > 0 ? "var(--ok)" : "var(--warn)";
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

  const onSendTx = () => {
    const to = sendToEl.current ? sendToEl.current.value.trim() : "";
    const amt = parseFloat(sendAmtEl.current ? sendAmtEl.current.value : "");
    if (!/^0x[0-9a-fA-F]{6,}$/.test(to)) return store.toast("Enter a destination address (0x…)");
    if (!(amt > 0)) return store.toast("Enter an amount");
    if (amt > s.liquid) return store.toast("Exceeds your liquid balance");
    store.requestSig({
      origin: "user wallet action",
      requester: "you · Wallet → Send",
      title: "Send " + fmt2(amt) + " SALT",
      rows: [
        { k: "To", v: to },
        { k: "Amount", v: fmt2(amt) + " SALT" },
        { k: "Route", v: "ERC-4337 UserOp · EntryPoint 0x077F…54Ef" },
      ],
      cost: "est. gas 0.0012 SALT",
      sponsor: sponsTxt,
      sponsorColor: sponsColor,
      apply: (h) => {
        store.setState((st) => ({ liquid: st.liquid - amt, sponsorUnits: Math.max(0, st.sponsorUnits - 1) }));
        store.addActivity("Send", "−" + fmt2(amt) + " SALT", h);
        if (sendToEl.current) sendToEl.current.value = "";
        if (sendAmtEl.current) sendAmtEl.current.value = "";
        store.toast("Sent — witnessed on 40204");
      },
    });
  };

  const onAddStake = () => {
    const amt = parseFloat(stakeAmtEl.current ? stakeAmtEl.current.value : "");
    if (!(amt > 0)) return store.toast("Enter an amount");
    if (amt > s.liquid) return store.toast("Exceeds your liquid balance");
    store.requestSig({
      origin: "user wallet action",
      requester: "you · Wallet → Staking",
      title: "Stake " + fmt2(amt) + " SALT",
      rows: [
        { k: "Action", v: "deposit(" + fmt2(amt) + ") → stSALT shares" },
        { k: "Contract", v: "LiquidStakingPool 0xfd27…685e" },
        { k: "Lockup", v: "withdrawals carry a 7-day lockup" },
      ],
      cost: "est. gas 0.0018 SALT",
      sponsor: sponsTxt,
      sponsorColor: sponsColor,
      apply: (h) => {
        store.setState((st) => ({ liquid: st.liquid - amt, selfStake: st.selfStake + amt, sponsorUnits: Math.max(0, st.sponsorUnits - 1) }));
        store.addActivity("Add stake", "−" + fmt2(amt) + " SALT", h);
        if (stakeAmtEl.current) stakeAmtEl.current.value = "";
        store.toast("Staked — position updated from chain");
      },
    });
  };

  const onUnstake = () => {
    const amt = parseFloat(unstakeAmtEl.current ? unstakeAmtEl.current.value : "");
    if (!(amt > 0)) return store.toast("Enter an amount");
    if (amt > s.selfStake) return store.toast("Only self-added stake can be withdrawn — granted principal is vaulted");
    const below = staked - amt < 32000;
    store.requestSig({
      origin: "user wallet action",
      requester: "you · Wallet → Staking",
      title: "Withdraw " + fmt2(amt) + " SALT",
      rows: [
        { k: "Action", v: "requestWithdraw(" + fmt2(amt) + ")" },
        { k: "Contract", v: "LiquidStakingPool 0xfd27…685e" },
        { k: "Unlocks", v: "2026-07-18 · 7-day lockup" },
      ],
      cost: "est. gas 0.0016 SALT",
      sponsor: "you pay gas — daily sponsorship budget reached for withdrawals",
      sponsorColor: "var(--warn)",
      warning: below
        ? "This drops your stake below 32,000 SALT — validator reward eligibility ends below the minimum."
        : "Funds unlock 2026-07-18. The 7-day lockup starts when this transaction settles.",
      apply: (h) => {
        store.setState((st) => ({ selfStake: st.selfStake - amt }));
        store.addActivity("Unstake · unlocks 2026-07-18", fmt2(amt) + " SALT", h);
        if (unstakeAmtEl.current) unstakeAmtEl.current.value = "";
        store.toast("Withdrawal queued — 7-day lockup running");
      },
    });
  };

  const activityRows = s.activity.map((a, i) => ({
    kind: a.kind,
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
            </div>
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
              <span className="mono" style={{ fontSize: 11, color: "var(--accent-text)", cursor: "pointer" }} title="Opens CitrateScan">
                {tx.hashShort} ↗
              </span>
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
              <span className="mono" style={{ fontSize: 11, color: "var(--accent-text)", cursor: "pointer" }}>
                chain proof ↗
              </span>
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
