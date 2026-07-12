import { useBlockNumber } from "wagmi";
import { LoaderMark } from "../components/LoaderMark";
import { Store } from "../shell/store";
import { AppState, nodeLabel } from "../shell/state";
import { citrate } from "../chain";
import { TUTORIALS } from "../data/seed";

const RANK: Record<string, number> = { free: 0, pilot: 1, enterprise: 2 };
const fmtI = (n: number) => Math.round(n).toLocaleString("en-US");
const fmt2 = (n: number) => n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
const short = (h: string) => (h ? h.slice(0, 6) + "…" + h.slice(-4) : "—");
const rel = (ts: number) => {
  const m = (Date.now() - ts) / 60000;
  if (m < 1) return "now";
  if (m < 60) return (m | 0) + "m";
  const h = m / 60;
  if (h < 24) return (h | 0) + "h";
  return ((h / 24) | 0) + "d";
};

const nodeColors: Record<string, string> = {
  off: "var(--tx-3)",
  prov: "#ffbd10",
  syncing: "#ffbd10",
  synced: "#8ecc09",
  paused: "#ffbd10",
  validating: "#8ecc09",
  error: "#dd7259",
};

/**
 * The ONE genuinely-live datum (Rule 1 / I-3): the network vitals "Height"
 * reads chain 40204's live block via wagmi useBlockNumber (viem publicClient
 * over https://rpc.citrate.ai). Loading and error states are honest — never a
 * mock. Everything else on this surface is prototype sim state, each captioned
 * with its real "Data source —" endpoint.
 */
export function Dashboard({ store, s }: { store: Store; s: AppState }) {
  const { data: liveHeight, isPending: heightPending, error: heightError } = useBlockNumber({ watch: true, chainId: citrate.id });

  const effTier = s.entitlement === "lapsed" ? "free" : s.tier;
  const staked = (s.hasGrant ? 32000 : 0) + s.selfStake;

  // Height vitals value: live chain read when node is off (reads fall back to
  // the public RPC in the design), sim height when the local node drives it.
  let heightValue: string;
  let heightSub: string;
  if (s.node === "off") {
    if (heightPending) {
      heightValue = "…";
      heightSub = "rpc.citrate.ai";
    } else if (heightError) {
      heightValue = "—";
      heightSub = "rpc unreachable";
    } else {
      heightValue = fmtI(Number(liveHeight));
      heightSub = "rpc.citrate.ai · live";
    }
  } else {
    heightValue = fmtI(s.height);
    heightSub = "local node";
  }

  const vitals = [
    { label: "Height", value: heightValue, sub: heightSub, color: "var(--tx-1)", tip: "eth_blockNumber", vsize: "21px" },
    { label: "Peers", value: s.node === "off" ? "—" : String(s.peers), sub: s.node === "off" ? "node off" : "net_peerCount", color: "var(--tx-1)", tip: s.node === "off" ? "Unknown — your node is off" : "net_peerCount", vsize: "21px" },
    { label: "Finality", value: Math.round(s.finAge) + "s", sub: "checkpoint age", color: "var(--tx-1)", tip: "BFT checkpoint every ~50 blocks", vsize: "21px" },
    { label: "Node", value: nodeLabel(s.node), sub: "supervisor", color: nodeColors[s.node], tip: "node-agent /status", vsize: "16px" },
    { label: "Staked", value: staked > 0 ? fmtI(staked) : "—", sub: staked > 0 ? "SALT" : "no stake", color: "var(--tx-1)", tip: "LiquidStakingPool shares", vsize: "21px" },
    { label: "Today", value: s.node === "validating" || s.earnToday > 0 ? fmt2(s.earnToday) : "—", sub: s.earnToday > 0 ? "SALT earned" : "not validating", color: s.earnToday > 0 ? "var(--accent-text)" : "var(--tx-3)", tip: "ContributionAccounting", vsize: "21px" },
  ];

  const chatBackendLabel = s.chatBackend === "gateway" && s.entitlement !== "lapsed" && effTier !== "free" ? "infer.citrate.ai · cgk_…7f2a" : "running on your machine";
  const chatDotColor = chatBackendLabel.indexOf("machine") >= 0 ? "#ffbd10" : "#8ecc09";

  const actRows = s.activity.map((a, i) => ({
    kind: a.kind,
    hashShort: short(a.hash),
    time: rel(a.ts),
    amount: a.amount,
    amtColor: a.amount && a.amount.indexOf("−") === 0 ? "var(--tx-1)" : "var(--accent-text)",
    rowClass: i === 0 && s.justSigned ? "cc-row-stroke" : "",
  }));
  const recentTx = actRows.slice(0, 4);
  const txEmpty = actRows.length === 0;
  const txMore = actRows.length > 4;

  const tutorials = TUTORIALS.map((t) => {
    const locked = RANK[t.minTier] > RANK[effTier];
    return {
      title: t.title,
      sub: t.minutes + " min · " + t.path,
      opacity: locked ? 0.55 : 1,
      cta: locked ? "members" : "open ↗",
      ctaColor: locked ? "var(--tx-3)" : "var(--accent-text)",
    };
  });

  const chatThinking = s.chatStatus === "thinking" || s.chatStatus === "tool";
  const chatThinkingLabel = s.chatStatus === "tool" ? "running tools" : "reasoning";
  const chatBusy = s.chatStatus !== "ready";
  const showSuggestions = s.chatMsgs.length <= 1 && s.chatStatus === "ready";
  const suggestions = ["What is my staking position?", "Break down my earnings", "Journal: node held through the night", "Network status"];

  const onSend = () => store.sendChat(store.chatInputEl ? store.chatInputEl.value : "");

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, minHeight: "100%", boxSizing: "border-box" }}>
      {/* vitals strip */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(6,1fr)", gap: 1, background: "var(--line-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-2)", overflow: "hidden" }}>
        {vitals.map((v) => (
          <div key={v.label} style={{ background: "var(--srf-1)", padding: "12px 14px", display: "flex", flexDirection: "column", gap: 4, minWidth: 0 }} title={v.tip}>
            <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)", whiteSpace: "nowrap" }}>
              {v.label}
            </span>
            <span className="tabular" style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: v.vsize, lineHeight: 1.3, color: v.color, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
              {v.value}
            </span>
            <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", whiteSpace: "nowrap" }}>
              {v.sub}
            </span>
          </div>
        ))}
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 316px", gap: 16, flex: 1, minHeight: 420 }}>
        {/* chat pane */}
        <div className="surface" style={{ display: "flex", flexDirection: "column", minHeight: 0, overflow: "hidden" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
            <span style={{ fontSize: 14, fontWeight: 500 }}>Agent</span>
            <span style={{ flex: 1 }}></span>
            <span className="mono" style={{ fontSize: 10, letterSpacing: ".08em", color: "var(--tx-3)", display: "inline-flex", alignItems: "center", gap: 6 }}>
              <span style={{ width: 6, height: 6, borderRadius: 999, background: chatDotColor }}></span>
              {chatBackendLabel}
            </span>
          </div>
          <div ref={(el) => { store.chatScrollEl = el; }} style={{ flex: 1, minHeight: 0, overflow: "auto", padding: 16, display: "flex", flexDirection: "column", gap: 16 }}>
            {s.chatMsgs.map((m) => (
              <div key={m.id} style={{ display: "flex", flexDirection: "column", gap: 5 }}>
                <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: m.who === "You" ? "var(--tx-3)" : "var(--accent-text)" }}>
                  {m.who}
                </span>
                <span style={{ fontSize: 13.5, lineHeight: 1.6, color: "var(--tx-1)", whiteSpace: "pre-wrap" }}>
                  {m.text}
                  {m.streaming && <span style={{ display: "inline-block", width: 7, height: 14, background: "var(--accent)", marginLeft: 2, verticalAlign: -2, animation: "ccCaret 1s step-end infinite" }}></span>}
                </span>
                {m.chips && m.chips.length > 0 && (
                  <span style={{ display: "flex", gap: 6, flexWrap: "wrap", marginTop: 2 }}>
                    {m.chips.map((c, i) => {
                      const bd = c.status === "declined" ? "var(--danger)" : c.status === "approved" ? "var(--ok)" : "var(--line-2)";
                      const fg = c.status === "declined" ? "var(--danger)" : c.status === "approved" ? "var(--ok)" : "var(--tx-2)";
                      return (
                        <span key={i} className="mono" style={{ fontSize: 10, letterSpacing: ".05em", padding: "2px 8px", borderRadius: 999, border: "1px solid " + bd, color: fg, background: "transparent" }}>
                          {c.label}
                        </span>
                      );
                    })}
                  </span>
                )}
              </div>
            ))}
            {chatThinking && (
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <span style={{ width: 34, height: 34, display: "inline-block", flexShrink: 0 }}>
                  <LoaderMark size={34} />
                </span>
                <span className="mono" style={{ fontSize: 10.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
                  {chatThinkingLabel}
                </span>
              </div>
            )}
          </div>
          {showSuggestions && (
            <div style={{ display: "flex", gap: 8, flexWrap: "wrap", padding: "0 16px 10px" }}>
              {suggestions.map((label) => (
                <button
                  key={label}
                  onClick={() => store.sendChat(label)}
                  style={{ fontFamily: "var(--font-sans)", fontSize: 12, color: "var(--tx-2)", background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: 999, padding: "5px 12px", cursor: "pointer" }}
                >
                  {label}
                </button>
              ))}
            </div>
          )}
          <div style={{ display: "flex", gap: 10, padding: "12px 16px", borderTop: "1px solid var(--line-1)" }}>
            <input
              ref={(el) => { store.chatInputEl = el; }}
              className="input"
              placeholder="Ask about your node, wallet, or memory…"
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  store.sendChat((e.target as HTMLInputElement).value);
                }
              }}
              style={{ flex: 1 }}
            />
            <button className="btn btn-primary" onClick={onSend} disabled={chatBusy}>
              Send
            </button>
          </div>
        </div>

        {/* right rail */}
        <div style={{ display: "flex", flexDirection: "column", gap: 16, minHeight: 0, overflow: "auto" }}>
          <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
            <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
              <span style={{ fontSize: 13.5, fontWeight: 500 }}>Recent transactions</span>
              <span className="mono" style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
                local node
              </span>
            </div>
            {txEmpty && (
              <p style={{ fontSize: 12.5, lineHeight: 1.55, color: "var(--tx-3)", margin: 0, padding: 16 }}>
                No transactions yet. When you send, stake, or claim, each signed action appears here — witnessed, linking to CitrateScan.
              </p>
            )}
            {recentTx.map((tx, i) => (
              <div key={i} className={tx.rowClass} style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 16px", borderBottom: "1px solid var(--line-1)" }}>
                <span style={{ flex: 1, minWidth: 0 }}>
                  <span style={{ display: "block", fontSize: 12.5, fontWeight: 500, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{tx.kind}</span>
                  <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)" }}>
                    {tx.hashShort} · {tx.time}
                  </span>
                </span>
                <span className="mono tabular" style={{ fontSize: 12, color: tx.amtColor, whiteSpace: "nowrap" }}>
                  {tx.amount}
                </span>
              </div>
            ))}
            {txMore && (
              <a
                href="#/wallet"
                onClick={(e) => {
                  e.preventDefault();
                  store.setState({ wTab: "activity" });
                  store.go("wallet");
                }}
                style={{ fontSize: 11.5, padding: "9px 16px", color: "var(--accent-text)", textDecoration: "none" }}
              >
                All activity →
              </a>
            )}
          </div>
          <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
            <div style={{ padding: "12px 16px", borderBottom: "1px solid var(--line-1)", fontSize: 13.5, fontWeight: 500 }}>Tutorials</div>
            {tutorials.map((tu, i) => (
              <div key={i} style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 16px", borderBottom: "1px solid var(--line-1)", opacity: tu.opacity }}>
                <span style={{ flex: 1, minWidth: 0 }}>
                  <span style={{ display: "block", fontSize: 12.5, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{tu.title}</span>
                  <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)" }}>
                    {tu.sub}
                  </span>
                </span>
                <span className="mono" style={{ fontSize: 10, color: tu.ctaColor, whiteSpace: "nowrap" }}>
                  {tu.cta}
                </span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
