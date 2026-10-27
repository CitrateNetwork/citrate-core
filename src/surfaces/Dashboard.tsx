import { useEffect } from "react";
import { useBlockNumber, useReadContract } from "wagmi";
import { PRODUCER_SAMPLE_BLOCKS, producerShortfall, producerWindowLabel, validatorDash } from "../shell/chainTruth";
import book from "../../src-tauri/addresses/40204.json";
import { AgentChat } from "../components/AgentChat";
import { Store } from "../shell/store";
import { AppState, nodeLabel } from "../shell/state";
import { citrate } from "../chain";
import { TUTORIALS } from "../data/seed";
import { federationUrl } from "../data/links";
import { WidgetsPanel } from "../widgets/WidgetsPanel";
import { DaemonsPanel } from "../daemons/DaemonsPanel";
import { appWidgetSources } from "../widgets/appSources";

const RANK: Record<string, number> = { free: 0, pilot: 1, enterprise: 2 };
const fmtI = (n: number) => Math.round(n).toLocaleString("en-US");
const short = (h: string) => (h ? h.slice(0, 6) + "…" + h.slice(-4) : "—");
const rel = (ts: number) => {
  const m = (Date.now() - ts) / 60000;
  if (m < 1) return "now";
  if (m < 60) return (m | 0) + "m";
  const h = m / 60;
  if (h < 24) return (h | 0) + "h";
  return ((h / 24) | 0) + "d";
};

// #66 — whole-network "Network validators" count. There's no all-p2p-node census on 40204
// (per DGX), but the active consensus validator set IS on-chain and reads identically from
// every node: ValidatorRegistry.activeCount() over the RPC already in the CSP. Honest source,
// no new infra/CSP. The address comes from the GENERATED book (a reroll moves it; a
// hard-coded copy here went stale once already). activeCount() selector 0x4331ed1f.
const VALIDATOR_REGISTRY = book.addresses.ValidatorRegistry as `0x${string}`;
const ACTIVE_COUNT_ABI = [
  { type: "function", name: "activeCount", stateMutability: "view", inputs: [], outputs: [{ type: "uint256" }] },
] as const;

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
 * The ONE genuinely-live datum (Rule 1 / I-3): the network vitals "Height"
 * reads chain 40204's live block via wagmi useBlockNumber (viem publicClient
 * over https://rpc.citrate.ai). Loading and error states are honest — never a
 * mock. Everything else on this surface is prototype sim state, each captioned
 * with its real "Data source —" endpoint.
 */
export function Dashboard({ store, s }: { store: Store; s: AppState }) {
  const { data: liveHeight, isPending: heightPending, error: heightError } = useBlockNumber({ watch: true, chainId: citrate.id });
  // #66 — the active validator set (whole-network, on-chain). Honest "—" until it reads.
  const { data: validatorCount } = useReadContract({
    address: VALIDATOR_REGISTRY,
    abi: ACTIVE_COUNT_ABI,
    functionName: "activeCount",
    chainId: citrate.id,
    query: { refetchInterval: 60_000 },
  });

  // The producer sample (cached 5 min in the store): shown next to the active count so the gap
  // between active and producing validators is visible (contract §2, §4).
  useEffect(() => {
    void store.refreshProducers();
  }, [store]);
  const shortfall = s.producers ? producerShortfall(s.producers) : null;
  const producingSub = s.producers && shortfall
    ? "producing: — · " + shortfall
    : s.producers
      ? s.producers.proposers.length + " producing · " + producerWindowLabel(s.producers)
      : s.producersError ? "producing: — · sample unavailable" : "producing: — · not sampled yet";

  const effTier = s.entitlement === "lapsed" ? "free" : s.tier;
  // C1-V V7: the ONE "staked" figure is ValidatorRegistry.stakeOf(pubkey) (UI truth contract §2),
  // folded as `bondedStake` from grantStatus. The membership grant sitting in a locked bond is not
  // staked until the bond is activated.
  const staked = s.bondedStake;
  // #348 review: the bond-locked word needs MemberBond.activated() = false; otherwise the one
  // validator state gives the word or "— · <reason>".
  const bondLockedNotStaked = s.hasGrant && !(staked > 0) && s.bondActivated === false;
  const validatorStateText = validatorDash(staked, s.validatorActivation, s.hasGrant, s.bondActivated);

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
  } else if (s.height > 0) {
    heightValue = fmtI(s.height);
    heightSub = "local node";
  } else {
    // C1-V V10: no reading before the first poll (and eth_blockNumber's 0x0 is not a height).
    heightValue = "—";
    heightSub = "local node · not read yet";
  }

  const vitals = [
    { label: "Height", value: heightValue, sub: heightSub, color: "var(--tx-1)", tip: "eth_blockNumber", vsize: "21px" },
    { label: "Peers", value: s.node === "off" ? "—" : String(s.peers), sub: s.node === "off" ? "node off" : "direct P2P", color: "var(--tx-1)", tip: s.node === "off" ? "Unknown — your node is off" : "Direct P2P peers your node is dialed into (bootnodes + sequencer + discovered members) — not the whole network's node count (net_peerCount)", vsize: "21px" },
    // C1-V V1: checkpoint finality is specified but not running on 40204 (UI truth contract §4).
    { label: "Finality", value: "—", sub: "not running yet", color: "var(--tx-3)", tip: "Checkpoint finality is specified but not running on betanet: the finalized tag still returns the genesis block. A confirmation count is a depth, not finality.", vsize: "21px" },
    { label: "Validators", value: validatorCount != null ? String(validatorCount) : "—", sub: (validatorCount != null ? "active · " : "active: reading… · ") + producingSub, color: "var(--tx-1)", tip: "Active: ValidatorRegistry.activeCount(), the active validator set (on-chain, the same from every node). Producing: distinct proposerPubkey over the last " + PRODUCER_SAMPLE_BLOCKS + " blocks on rpc.citrate.ai. Active does not imply producing.", vsize: "21px" },
    { label: "Node", value: nodeLabel(s.node), sub: "supervisor", color: nodeColors[s.node], tip: "node-agent /status", vsize: "16px" },
    {
      label: "Staked",
      value: staked > 0 ? fmtI(staked) : bondLockedNotStaked ? "0" : "—",
      sub: staked > 0 ? "SALT · stakeOf" : validatorStateText,
      color: "var(--tx-1)",
      tip: "ValidatorRegistry.stakeOf(pubkey): the only staked figure. A grant in a locked bond is not validator stake.",
      vsize: "21px",
    },
    // C1-V V8: the "Today" tile is gone. No contract or RPC gives a per-day reward figure
    // (UI truth contract §1 Rewards), so there is nothing to show; rewards live on Node > Earning.
  ];

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
      locked,
      minutes: t.minutes,
      url: federationUrl(t.path),
    };
    // Sort: unlocked first, then shortest first (audit: tutorials need ordering).
  }).sort((a, b) => (a.locked === b.locked ? a.minutes - b.minutes : a.locked ? 1 : -1));

  // The agent chat (transcript + model picker + voice + auto-scroll) now lives in the shared
  // <AgentChat> component, mounted here and on the Agent surface (#62).

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, minHeight: "100%", boxSizing: "border-box" }}>
      {/* vitals strip */}
      <div style={{ display: "grid", gridTemplateColumns: `repeat(${vitals.length},1fr)`, gap: 1, background: "var(--line-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-2)", overflow: "hidden" }}>
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

      {/* rc.5 FIX 4: items align to the start, so the chat sizes itself to the window instead of
          stretching to the right rail's height (which pushed the composer below the fold). */}
      <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 316px", gap: 16, alignItems: "start" }}>
        {/* chat pane — shared component, also mounted on the Agent surface (#62) */}
        <AgentChat store={store} s={s} />

        {/* right rail */}
        <div style={{ display: "flex", flexDirection: "column", gap: 16, minHeight: 0, overflow: "auto" }}>
          {/* HUP-S10.3 — widgets (sandboxed tiles) and daemons (scheduled tasks inside a budget) */}
          <WidgetsPanel sources={appWidgetSources} />
          <DaemonsPanel />
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
                {tu.locked ? (
                  <span className="mono" style={{ fontSize: 10, color: tu.ctaColor, whiteSpace: "nowrap" }}>
                    {tu.cta}
                  </span>
                ) : (
                  <button
                    className="mono"
                    onClick={() => void store.openExternal(tu.url)}
                    style={{ fontSize: 10, color: tu.ctaColor, whiteSpace: "nowrap", background: "none", border: "none", cursor: "pointer", padding: 0 }}
                  >
                    {tu.cta}
                  </button>
                )}
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
