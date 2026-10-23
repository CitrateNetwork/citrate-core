// =====================================================================
// citrate-core — Node › Status (core plan K3; design slice 2 "Node status")
//
// One sentence in UI truth contract §1 words, Start and Stop, then what this node reads about itself
// and what the network reads say, each row naming its source (Rule 7):
//   - node word: nodeWords.ts (node_status via Store.refreshNode, public eth_blockNumber, genesis check);
//   - registered validators: ValidatorRegistry.activeCount() (contract v1.1 §2: it includes
//     registrations still pending activation, so it is "Registered", never "Active");
//   - validator word and eligible validators: C1-V's chainTruth reads folded by the store
//     (`s.validatorActivation`, `s.memberBond`, `s.bondedStake`, `s.validatorCounts`); "—" with the
//     reason until read, and the validator word is left out of the sentence until then;
//   - Activate validator: offered only through #366's one gate, canOfferActivation (activationOffer
//     in nodeWords.ts), on a page that reads Synced; otherwise the card says the gate's reason;
//   - freshness: `s.nodeStatusAt`, the node_status answer the store's poll committed. The 10 s
//     probe only reports that node_status fails; its answers never make the store's values current.
//   - consensus validator set: no RPC exposes the epoch snapshot, so no number (contract v1.1 §2).
// No sync percentage, no finality, no local height that ticks on its own.
// =====================================================================
import { useEffect, useRef, useState } from "react";
import { useBlockNumber, useReadContract } from "wagmi";
import book from "../../../src-tauri/addresses/40204.json";
import { citrate } from "../../chain";
import { bridge } from "../../bridge";
import { BRIDGE_MODE } from "../../bridge/mode";
import type { Store } from "../../shell/store";
import type { AppState } from "../../shell/state";
import { useNetworkCompat, type NetworkCompat } from "../../shell/networkCompat";
import { validatorDash, validatorWord } from "../../shell/chainTruth";
import { FRESH_MS, activationOffer, headFresh, directPeers, heightLine, nodeWord, statusSentence, TONE_COLOR } from "./nodeWords";
import { PAGE, Panel, Row, StateCard, errText } from "./parts";

const VALIDATOR_REGISTRY = book.addresses.ValidatorRegistry as `0x${string}`;
const ACTIVE_COUNT_ABI = [{ type: "function", name: "activeCount", stateMutability: "view", inputs: [], outputs: [{ type: "uint256" }] }] as const;
/** How often the tab checks that node_status still answers (its values and their age come from the store's 2 s poll). */
const STATUS_PROBE_MS = 10_000;
/** How often read ages are re-evaluated, so a read going stale shows without another poll. */
const AGE_TICK_MS = 5_000;

const fmtI = (n: number) => Math.round(n).toLocaleString("en-US");

export interface NodeStatusViewProps {
  store: Store;
  s: AppState;
  /** Public eth_blockNumber; null until read. */
  publicHead: number | null;
  /** Age of the last good public head read (ms); null = unknown. */
  publicHeadAgeMs: number | null;
  /** The latest public head poll failed. */
  publicHeadError: boolean;
  /** Age of the node_status answer behind the store's values (`s.nodeStatusAt`, ms); null = none yet. */
  statusAgeMs: number | null;
  compat: NetworkCompat;
  mode: "tauri" | "sim";
  /** The last node_status failure, or null while it answers. */
  statusError: string | null;
  /** ValidatorRegistry.activeCount(); null until read. */
  registered?: number | null;
}

/** Node › Status with its live reads. */
export function NodeStatus({ store, s }: { store: Store; s: AppState }) {
  const { data: head, dataUpdatedAt, isError: headError } = useBlockNumber({ watch: true, chainId: citrate.id });
  const now = useNow(AGE_TICK_MS);
  const { status: compat } = useNetworkCompat();
  const { data: registered } = useReadContract({
    address: VALIDATOR_REGISTRY,
    abi: ACTIVE_COUNT_ABI,
    functionName: "activeCount",
    chainId: citrate.id,
    query: { refetchInterval: 60_000 },
  });
  const [statusError, setStatusError] = useState<string | null>(null);
  useEffect(() => {
    if (BRIDGE_MODE !== "tauri") return;
    let live = true;
    // Only failure is taken from the probe. Its answer is not what the page shows, so a success here
    // must not make the store's values look current (Codex on #362): their age is s.nodeStatusAt.
    const probe = () =>
      bridge.node
        .status()
        .then(() => live && setStatusError(null))
        .catch((e) => live && setStatusError(errText(e)));
    void probe();
    const t = setInterval(probe, STATUS_PROBE_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, []);
  return (
    <NodeStatusView
      store={store}
      s={s}
      publicHead={head != null ? Number(head) : null}
      publicHeadAgeMs={head != null && dataUpdatedAt > 0 ? Math.max(0, now - dataUpdatedAt) : null}
      publicHeadError={headError}
      statusAgeMs={s.nodeStatusAt == null ? null : Math.max(0, now - s.nodeStatusAt)}
      compat={compat}
      mode={BRIDGE_MODE}
      statusError={statusError}
      registered={registered != null ? Number(registered) : null}
    />
  );
}

/** The current time, re-read every `ms`. */
function useNow(ms: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(t);
  }, [ms]);
  return now;
}

/** How long `value` has stayed the same while `active` (ms); 0 while inactive. */
function useUnchangedFor(value: number, active: boolean): number {
  const last = useRef<{ v: number; at: number } | null>(null);
  const [, tick] = useState(0);
  if (!active) last.current = null;
  else if (!last.current || last.current.v !== value) last.current = { v: value, at: Date.now() };
  useEffect(() => {
    if (!active) return;
    const t = setTimeout(() => tick((n) => n + 1), FRESH_MS + 1_000);
    return () => clearTimeout(t);
  }, [active, value]);
  return last.current ? Date.now() - last.current.at : 0;
}

/** How long the local peer count has read 0 while the node answers RPC. */
function usePeersZeroFor(zero: boolean): number {
  const since = useRef<number | null>(null);
  const [, tick] = useState(0);
  if (zero && since.current == null) since.current = Date.now();
  if (!zero) since.current = null;
  // Re-render once the 60 s window can have passed, so "No peers" appears without another poll.
  useEffect(() => {
    if (!zero) return;
    const t = setTimeout(() => tick((n) => n + 1), 61_000);
    return () => clearTimeout(t);
  }, [zero]);
  return since.current == null ? 0 : Date.now() - since.current;
}

export function NodeStatusView({
  store,
  s,
  publicHead,
  publicHeadAgeMs,
  publicHeadError,
  statusAgeMs,
  compat,
  mode,
  statusError,
  registered = null,
}: NodeStatusViewProps) {
  const answering = s.height > 0 && s.node !== "off" && s.node !== "error";
  const peersZeroForMs = usePeersZeroFor(answering && s.peers === 0);
  const localStillForMs = useUnchangedFor(s.height, answering);

  if (mode !== "tauri") {
    return (
      <div data-node-tab="status" style={PAGE}>
        <StateCard
          kind="unavailable"
          title="The node needs the desktop app"
          body="Node status comes from the app's supervisor. The web preview has none, so nothing is shown rather than invented rows."
          why="node.status() → Unavailable (BRIDGE_MODE = sim)"
        />
      </div>
    );
  }
  if (statusError) {
    return (
      <div data-node-tab="status" style={PAGE}>
        <StateCard
          kind="error"
          title="Couldn't read the node's status"
          body="Core didn't answer the status read, so every number on this page is unknown, not zero, and no word is guessed. The page reads again every 10 seconds."
          why={"node_status → error: " + statusError}
        />
      </div>
    );
  }
  // A paused node is not polled on purpose (Store.refreshNode), so its age says nothing; it reads Off.
  if (s.node !== "paused" && statusAgeMs != null && statusAgeMs > FRESH_MS) {
    return (
      <div data-node-tab="status" style={PAGE}>
        <StateCard
          kind="error"
          title="The node's status is out of date"
          body="node_status last answered more than 60 seconds ago, so its height, peers and state are unknown, not current. The page reads again every 10 seconds."
          why={"node_status → last answer " + Math.round(statusAgeMs / 1000) + " s ago"}
        />
      </div>
    );
  }

  const w = nodeWord({
    node: s.node,
    blocked: s.nodeBlocked,
    compat,
    localHeight: s.height,
    publicHead,
    publicHeadAgeMs,
    publicHeadError,
    peers: s.peers,
    peersZeroForMs,
    localStillForMs,
  });
  const headOk = headFresh({ publicHead, publicHeadAgeMs, publicHeadError });
  // C1-V (contract v1.1 §1): the validator word from the registry and bond reads only.
  const vWord = validatorWord(s.bondedStake, s.validatorActivation, s.hasGrant, s.memberBond);
  const vDash = validatorDash(s.bondedStake, s.validatorActivation, s.hasGrant, s.memberBond);
  const sentence = statusSentence(w, vWord);
  const canStart = s.node === "off" || s.node === "error" || s.node === "paused";
  const notice = noticeFor(w.word, store);

  return (
    <div data-node-tab="status" style={{ ...PAGE, display: "grid", gridTemplateColumns: "minmax(0,1fr) 300px", alignItems: "start" }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 16, minWidth: 0 }}>
        <div className="surface" style={{ padding: "20px 22px", display: "flex", flexDirection: "column", gap: 12 }}>
          <span className="eyebrow">Status</span>
          <span style={{ display: "flex", alignItems: "center", gap: 12 }}>
            <span style={{ width: 10, height: 10, borderRadius: 999, background: TONE_COLOR[w.tone], flexShrink: 0 }}></span>
            <span data-testid="node-sentence" style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 26, lineHeight: 1.2 }}>
              {sentence}
            </span>
          </span>
          <span style={{ fontSize: 13, lineHeight: 1.6, color: "var(--tx-2)", overflowWrap: "anywhere" }}>{w.why}</span>
          {notice && (
            <div
              data-testid="node-notice"
              style={{ display: "flex", flexDirection: "column", gap: 8, padding: "12px 14px", borderRadius: "var(--r-2)", border: "1px solid " + (notice.red ? "var(--danger)" : "var(--warn)"), background: notice.red ? "var(--danger-bg)" : "var(--warn-bg)" }}
            >
              <span style={{ fontSize: 13.5, fontWeight: 500 }}>{notice.title}</span>
              <span style={{ fontSize: 12.5, lineHeight: 1.55, color: "var(--tx-2)" }}>{notice.body}</span>
              {notice.cta && (
                <span>
                  <button className="btn btn-secondary btn-sm" onClick={notice.cta.go}>
                    {notice.cta.label}
                  </button>
                </span>
              )}
            </div>
          )}
          {s.miningHold && <span style={{ fontSize: 12.5, color: "var(--tx-2)", overflowWrap: "anywhere" }}>{s.miningHold}</span>}
          <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
            {canStart ? (
              <button className="btn btn-primary btn-sm" onClick={() => store.startNode()}>
                Start node
              </button>
            ) : (
              <button className="btn btn-ghost btn-sm" onClick={() => store.stopNode()} disabled={s.node === "prov"}>
                Stop node
              </button>
            )}
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
              node_start / node_stop · the node keeps running when you close the window
            </span>
          </div>
        </div>

        <Panel title="This node" caption="local reads">
          <Row k="State" v={w.word} src="node_status (supervisor state, local eth_blockNumber) and the public head (contract §1)" />
          <Row k="Direct peers" v={directPeers(answering, s.height, s.peers)} src="local net_peerCount; unknown, not 0, while the node isn't answering" vColor={answering ? undefined : "var(--tx-3)"} />
          <Row k="Height" v={heightLine(s.height, headOk ? publicHead : null)} src="local eth_blockNumber against a public eth_blockNumber on rpc.citrate.ai read in the last 60 s" />
          <Row
            k="Validator"
            testid="validator-word"
            v={vDash}
            vColor={vWord ? undefined : "var(--tx-3)"}
            src="ValidatorRegistry.validatorInfo(pubkey) status and actEpoch against currentEpoch(), isActive(pubkey), and MemberBond.activated() / isUnlocked() (contract v1.1 §1). Stake amounts never decide this word"
          />
          <Row k="Producing blocks" v="— · not read" vColor="var(--tx-3)" src="a recent block with proposerPubkey = this node's key. Validator active does not imply this" />
        </Panel>

        <Panel title="Network" caption="public RPC and ValidatorRegistry">
          <Row
            k="Network"
            v={compat === "compatible" ? "Citrate betanet" : compat === "retired" ? "a different network than this build" : "— · genesis not checked yet"}
            vColor={compat === "retired" ? "var(--danger)" : compat === "unknown" ? "var(--tx-3)" : undefined}
            src="eth_chainId 0x9d0c and the genesis hash this build ships with (contract §6); chain id alone is not enough"
          />
          <Row
            k="Network head"
            v={headOk && publicHead != null && publicHead > 0 ? fmtI(publicHead) : "— · network head unavailable"}
            vColor={headOk ? undefined : "var(--tx-3)"}
            src="public eth_blockNumber on rpc.citrate.ai; a failed poll, 0x0 or a read older than 60 s is unavailable"
          />
          <Row
            k="Registered validators"
            v={registered != null ? fmtI(registered) : "—"}
            src="ValidatorRegistry.activeCount(). Includes registrations still pending activation, so it is not “active”"
          />
          <Row
            k="Eligible validators"
            testid="eligible-validators"
            v={s.validatorCounts ? fmtI(s.validatorCounts.eligible) : "— · not read yet"}
            vColor={s.validatorCounts ? undefined : "var(--tx-3)"}
            src="activeSet() members with actEpoch ≤ currentEpoch(): the live registry, not consensus membership"
          />
          <Row k="Consensus validator set" v="unavailable until a snapshot RPC exists" vColor="var(--tx-3)" src="the governing epoch snapshot; no RPC exposes it yet, so no number is shown" />
        </Panel>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <ActivateCard store={store} s={s} pageWord={w.word} />
        <div className="surface" style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
          <span className="eyebrow">Words on this page</span>
          <span style={{ fontSize: 12, lineHeight: 1.6, color: "var(--tx-2)" }}>
            Synced means this node is within 5 blocks of the public head. Validator active does not mean producing blocks. Checkpoint finality is not running yet, so this page counts confirmations, never finality.
          </span>
          <span style={{ fontSize: 12, lineHeight: 1.6, color: "var(--tx-2)" }}>
            Services are the processes this app supervises; they are on the{" "}
            <a href="#/node/services" onClick={(e) => (e.preventDefault(), store.go("node/services"))}>
              Services
            </a>{" "}
            tab.
          </span>
        </div>
      </div>
    </div>
  );
}

/** The notice under the sentence for the words that need an action. */
function noticeFor(word: string, store: Store): { title: string; body: string; red: boolean; cta?: { label: string; go: () => void } } | null {
  switch (word) {
    case "Update required":
      return {
        title: "Update required",
        body: "This build is for a different genesis than the network it is talking to. At betanet genesis the network moves to a new chain; updating keeps your keys, wallet and settings.",
        red: true,
        cta: { label: "Open Advanced", go: () => store.go("node/advanced") },
      };
    case "Sync stalled":
      return {
        title: "Sync stalled",
        body: "Citrate keeps the node running and keeps asking peers. If it stays stalled, read the full log, or restart the node from Services.",
        red: false,
        cta: { label: "Open the full log", go: () => store.go("node/advanced") },
      };
    case "No peers":
      return {
        title: "No direct peers",
        body: "Check this computer's connection and firewall. A node on an old genesis also shows no peers, because peers refuse its handshake.",
        red: false,
        cta: { label: "Open Services", go: () => store.go("node/services") },
      };
    case "Error":
      return {
        title: "The node didn't start",
        body: "The reason is above. Crash records and the full log are on the Advanced tab.",
        red: true,
        cta: { label: "Open crash records", go: () => store.go("node/advanced") },
      };
    default:
      return null;
  }
}

/**
 * Activate validator (moved from the 0.5.1 Node screen): on a synced node, link this device's wallet
 * first, then register the membership bond through the signature ceremony (store.activateValidator,
 * which checks canOfferActivation again before arming). Offered only when activationOffer allows it,
 * that is #366's canOfferActivation on a page that reads Synced; otherwise the card says why. No
 * amount is shown: the minimum stake is read by C1-V, never hard-coded here.
 */
function ActivateCard({ store, s, pageWord }: { store: Store; s: AppState; pageWord: string }) {
  const offer = activationOffer(pageWord, s);
  if (!offer.offer) {
    return (
      <div className="surface" style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
        <span className="eyebrow">Validator</span>
        <span data-testid="activate-gate" style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)", overflowWrap: "anywhere" }}>
          {offer.why}
        </span>
      </div>
    );
  }
  const linked = store.walletIsLinked();
  return (
    <div className="surface" style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
      <span className="eyebrow">Validator</span>
      {!linked ? (
        <>
          <p style={{ fontSize: 12, color: "var(--tx-2)", margin: 0, lineHeight: 1.5 }}>
            Before activating, link this device's wallet to your Citrate identity. Your membership bond is paid to the wallet your identity names, so it must be this one.
          </p>
          <button className="btn btn-primary btn-sm" style={{ alignSelf: "flex-start" }} onClick={() => void store.linkWallet()}>
            Link this wallet · no funds move
          </button>
        </>
      ) : (
        <>
          <p style={{ fontSize: 12, color: "var(--tx-2)", margin: 0, lineHeight: 1.5 }}>
            Activating registers your membership bond with the validator registry. You approve it in the signature ceremony. A registered validator becomes active from a later epoch.
          </p>
          <button className="btn btn-primary btn-sm" style={{ alignSelf: "flex-start" }} onClick={() => void store.activateValidator()}>
            Activate validator
          </button>
        </>
      )}
    </div>
  );
}
