// =====================================================================
// citrate-core: Node › Status (core plan K3; design slice 2 "Node status")
//
// One sentence in UI truth contract §1 words, Start and Stop, then what this node reads about itself
// and what the network reads say, each row naming its source (Rule 7):
//   - node word: surfaces/node/nodeWords.ts (node_status via Store.refreshNode, public
//     eth_blockNumber, genesis check against the active network profile);
//   - validator word: the store's registry and bond reads (chainTruth validatorWord: registry status
//     and actEpoch, ValidatorRegistry.stakeOf, MemberBond.activated() / isUnlocked()); left out of
//     the sentence until read, "—" with the reason in its row;
//   - your stake: ValidatorRegistry.stakeOf(pubkey), the one staked figure (#348);
//   - producing: the store's producer sample (distinct proposerPubkey over the sampled window)
//     against the member's validator key (MemberBond.pubkey() once activated, C1-V5);
//   - registered / eligible validators: the store's registry read (activeSet() and each member's
//     actEpoch against currentEpoch(); contract v1.1 §2), "—" until read;
//   - Activate validator: offered only through the one gate, canOfferActivation (activationOffer
//     in nodeWords.ts), on a page that reads Synced; a recorded activation shows its status with
//     Try again / Forget whatever the gate says (#406), and the store re-runs the full gate on a
//     fresh read before signing (#405);
//   - freshness: `s.nodeStatusAt`, the node_status answer the store's poll committed. The 10 s
//     probe only reports that node_status fails; its answers never make the store's values current.
// Logs, the last crash record and the network identity are on Node › Advanced; validator rewards
// are on Node › Rewards; the supervised processes are on Node › Services.
// No sync percentage, no finality, no CPU / memory / disk figures (no read exists), no seeded rows.
// =====================================================================
import { useEffect, useRef, useState } from "react";
import { useBlockNumber } from "wagmi";
import { citrate } from "../chain";
import { bridge } from "../bridge";
import { BRIDGE_MODE } from "../bridge/mode";
import type { Store } from "../shell/store";
import type { AppState } from "../shell/state";
import { fmtSaltFromWei } from "../shell/state";
import { useNetworkCompat, type NetworkCompat } from "../shell/networkCompat";
import { shortNet } from "../shell/netScope";
import { CONSENSUS_LAG_NOTE, UNACTIVATED_WITHDRAW_NOTE, producerShortfall, producerWindowLabel, producingWord, readStakeSalt, validatorDash, validatorWord } from "../shell/chainTruth";
import { grantExplanation } from "../data/grantCopy";
import { activateValidatorLabel } from "../onboarding/activateValidator";
import { ActivationTxPanel, hasActivationTxStatus } from "../onboarding/ActivationTxPanel";
import { SurfaceProps } from "./shared";
import { FRESH_MS, TONE_COLOR, activationOffer, bondFlags, directPeers, headFresh, heightLine, nodeWord, statusSentence } from "./node/nodeWords";
import { PAGE, Panel, Row, StateCard, errText } from "./node/parts";

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
}

/** Node › Status with its live reads. */
export function Node({ store, s }: SurfaceProps) {
  const { data: head, dataUpdatedAt, isError: headError } = useBlockNumber({ watch: true, chainId: citrate.id });
  const now = useNow(AGE_TICK_MS);
  const { status: compat } = useNetworkCompat();
  const [statusError, setStatusError] = useState<string | null>(null);
  // The producer sample and the registry counts (cached 5 min in the store).
  useEffect(() => {
    void store.refreshProducers();
  }, [store]);
  // #406: read the activation (and the durable record of a sent one) on mount, so a recorded
  // activation shows here with its way out (Try again / Forget), as on S7.
  useEffect(() => {
    if (BRIDGE_MODE === "tauri" && s.hasGrant) void store.refreshActivation();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [s.hasGrant]);
  useEffect(() => {
    if (BRIDGE_MODE !== "tauri") return;
    let live = true;
    // Only failure is taken from the probe. Its answer is not what the page shows, so a success here
    // must not make the store's values look current: their age is s.nodeStatusAt.
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
    const t = setTimeout(() => tick((n) => n + 1), FRESH_MS + 1_000);
    return () => clearTimeout(t);
  }, [zero]);
  return since.current == null ? 0 : Date.now() - since.current;
}

export function NodeStatusView({ store, s, publicHead, publicHeadAgeMs, publicHeadError, statusAgeMs, compat, mode, statusError }: NodeStatusViewProps) {
  const answering = s.height > 0 && s.node !== "off" && s.node !== "error" && s.node !== "paused";
  const peersZeroForMs = usePeersZeroFor(answering && s.peers === 0);
  const localStillForMs = useUnchangedFor(s.height, answering);

  if (mode !== "tauri") {
    return (
      <div data-node-tab="status" style={PAGE}>
        <StateCard
          kind="unavailable"
          title="The node needs the desktop app"
          body="Node status comes from the app's supervisor. The web preview has none, so nothing is shown rather than invented rows."
          why="node_status → Unavailable (BRIDGE_MODE = sim)"
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
  const bond = bondFlags(s);
  // C1-V (contract v1.1 §1): the validator word from the registry and bond reads only.
  const vWord = validatorWord(s.bondedStake, s.validatorActivation, s.hasGrant, bond.activated, bond.unlocked);
  const vDash = validatorDash(s.bondedStake, s.validatorActivation, s.hasGrant, bond.activated, bond.unlocked);
  const sentence = statusSentence(w, vWord);
  const producing = producingLine(s);
  const canStart = s.node === "off" || s.node === "error" || s.node === "paused";
  const notice = noticeFor(w.word, store);
  // The stake figure itself only once stakeOf has read this session (unknown is not zero, #555).
  const stakeRead = readStakeSalt(s);
  // RC52: the genesis-switch notice stays here until the member dismisses it (a toast is easy to
  // miss). `switched` is the webview's own finding: its saved chain values were another network's.
  const netNotice = s.networkNotice;
  const switched = netNotice !== null && netNotice.from !== netNotice.to;

  return (
    <div data-node-tab="status" style={{ ...PAGE, display: "grid", gridTemplateColumns: "minmax(0,1fr) 300px", alignItems: "start" }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 16, minWidth: 0 }}>
        {netNotice && (
          <div
            role="status"
            data-network-notice="switch"
            className="surface"
            style={{ padding: "12px 16px", borderLeft: "3px solid var(--warn)", display: "flex", flexDirection: "column", gap: 6 }}
          >
            <span style={{ fontSize: 13.5, fontWeight: 500 }}>This app is now on a new network</span>
            {switched && (
              <span style={{ fontSize: 12.5, color: "var(--tx-2)", overflowWrap: "anywhere" }}>
                Values read on the previous network (genesis {shortNet(netNotice.from)}): the grant, bond, balances, claimable
                and transaction history, are not shown as current. Each is read again from this network, and shows
                &quot;not read yet&quot; until then.{" "}
                {netNotice.archived
                  ? "The previous network's values are kept on this device (Wallet, Activity, under Previous network)."
                  : "The previous network's values could not be kept on this device (storage unavailable)."}
              </span>
            )}
            {netNotice.node && <span style={{ fontSize: 12.5, color: "var(--tx-2)", overflowWrap: "anywhere" }}>{netNotice.node}</span>}
            <span>
              <button className="btn btn-secondary btn-sm" onClick={() => store.dismissNetworkNotice()}>
                Dismiss
              </button>
            </span>
          </div>
        )}
        <div className="surface" style={{ padding: "20px 22px", display: "flex", flexDirection: "column", gap: 12 }}>
          <span className="eyebrow">Status</span>
          <span style={{ display: "flex", alignItems: "center", gap: 12 }}>
            <span style={{ width: 10, height: 10, borderRadius: 999, background: TONE_COLOR[w.tone], flexShrink: 0 }}></span>
            <span data-testid="node-sentence" style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 26, lineHeight: 1.2 }}>
              {sentence}
            </span>
          </span>
          {/* SCL-S0.6: a refused start names the holder (pid, path) and what to do, from node.rs. */}
          <span role={s.nodeBlocked ? "alert" : undefined} style={{ fontSize: 13, lineHeight: 1.6, color: "var(--tx-2)", overflowWrap: "anywhere" }}>
            {w.why}
          </span>
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
          {/* The mining gate (node.rs) is holding block production; the reason says why. */}
          {s.miningHold && (
            <span role="status" data-testid="mining-hold" style={{ fontSize: 12.5, color: "var(--tx-2)", overflowWrap: "anywhere" }}>
              {s.miningHold}
            </span>
          )}
          <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
            {/* Start / Stop drive the real supervisor (bridge.node.start / stop). There is no pause:
                no supervisor pause exists, so none is drawn. */}
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
          <Row
            k="Direct peers"
            testid="direct-peers"
            v={directPeers(answering, s.height, s.peers)}
            src="local net_peerCount; unknown, not 0, while the node isn't answering. Per-peer detail has no read yet"
            vColor={answering ? undefined : "var(--tx-3)"}
          />
          <Row k="Height" v={heightLine(s.height, headOk ? publicHead : null)} src="local eth_blockNumber against a public eth_blockNumber read in the last 60 s" />
          <Row
            k="Validator"
            testid="validator-word"
            v={
              vWord === "Validator active" ? (
                <>
                  {vDash}
                  <span style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)" }}>{CONSENSUS_LAG_NOTE}</span>
                </>
              ) : vWord === "Unlocked · not activated" ? (
                <>
                  {vDash}
                  {/* Contract v1.2 §1: the one detail for an unlocked, never-activated bond. */}
                  <span style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)" }}>{UNACTIVATED_WITHDRAW_NOTE}</span>
                </>
              ) : (
                vDash
              )
            }
            vColor={vWord === "Validator active" ? "var(--ok)" : vWord ? undefined : "var(--tx-3)"}
            src="ValidatorRegistry.isActive(pubkey), validatorInfo(pubkey).actEpoch, currentEpoch() and stakeOf(pubkey), and MemberBond.activated() / isUnlocked() (contract v1.1 §1)"
          />
          <Row
            k="Your stake"
            testid="your-stake"
            v={stakeRead != null && stakeRead > 0 ? fmtI(stakeRead) + " SALT" : stakeRead == null ? "— · not read yet" : "—"}
            vColor={stakeRead != null && stakeRead > 0 ? undefined : "var(--tx-3)"}
            src="ValidatorRegistry.stakeOf(pubkey): the only staked figure. A grant in a locked bond is not validator stake"
          />
          <Row
            k="Producing"
            testid="producing-blocks"
            v={
              producing.window ? (
                <>
                  {producing.text}
                  <span style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)" }}>{producing.window}</span>
                </>
              ) : (
                producing.text
              )
            }
            vColor={producing.known ? undefined : "var(--tx-3)"}
            src="distinct proposerPubkey in the sampled blocks (its window is stated with the word), against this member's validator key. Validator active does not imply this"
          />
        </Panel>

        <Panel title="Network" caption="public RPC and ValidatorRegistry">
          <Row
            k="Network"
            v={compat === "compatible" ? "Citrate betanet" : compat === "retired" || compat === "unset" ? "Update required" : "— · genesis not checked yet"}
            vColor={compat === "retired" || compat === "unset" ? "var(--danger)" : compat === "unknown" ? "var(--tx-3)" : undefined}
            src="eth_chainId and the genesis hash of this build's network profile (contract §6); chain id alone is not enough"
          />
          <Row
            k="Network head"
            v={headOk && publicHead != null ? fmtI(publicHead) : "— · network head unavailable"}
            vColor={headOk ? undefined : "var(--tx-3)"}
            src="public eth_blockNumber; a failed poll, 0x0 or a read older than 60 s is unavailable"
          />
          <Row
            k="Registered validators"
            testid="registered-validators"
            v={s.validatorCounts ? fmtI(s.validatorCounts.registered) : "— · not read yet"}
            vColor={s.validatorCounts ? undefined : "var(--tx-3)"}
            src="ValidatorRegistry.activeSet() members. Includes registrations still pending activation, so it is not “active”"
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
        <ValidatorCard store={store} s={s} pageWord={w.word} />
        <div className="surface" style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
          <span className="eyebrow">Words on this page</span>
          <span style={{ fontSize: 12, lineHeight: 1.6, color: "var(--tx-2)" }}>
            Synced means this node is within 5 blocks of the public head. Validator active does not mean producing blocks. Checkpoint finality is not running yet, so this page counts confirmations, never finality.
          </span>
          <span style={{ fontSize: 12, lineHeight: 1.6, color: "var(--tx-2)" }}>
            The processes this app supervises are on{" "}
            <a href="#/node/services" onClick={(e) => (e.preventDefault(), store.go("node/services"))}>
              Services
            </a>
            ; the node's log and its last crash record are on{" "}
            <a href="#/node/advanced" onClick={(e) => (e.preventDefault(), store.go("node/advanced"))}>
              Advanced
            </a>
            .
          </span>
        </div>
      </div>
    </div>
  );
}

/**
 * The "Producing" row (contract §1/§2, C1-V review): seen among the sampled proposers, not seen
 * ("Not seen producing", never "not producing"), or "—" with why there is no word. Registered stake
 * is required: an unregistered key has nothing to produce. There is no per-proposer count, so none
 * is shown.
 */
export function producingLine(s: Pick<AppState, "bondedStake" | "producers" | "producersError" | "validatorPubkey">): { text: string; known: boolean; window: string | null } {
  if (!(s.bondedStake > 0)) return { text: "— · not a registered validator", known: false, window: null };
  const word = producingWord(s.producers, s.validatorPubkey);
  if (word && s.producers) return { text: word, known: true, window: producerWindowLabel(s.producers) };
  const short = s.producers ? producerShortfall(s.producers) : null;
  if (short) return { text: "— · producer sample incomplete: " + short, known: false, window: null };
  if (s.producersError) return { text: "— · producer sample unavailable: " + s.producersError, known: false, window: null };
  if (!s.validatorPubkey) return { text: "— · validator key not read yet", known: false, window: null };
  return { text: "— · not sampled yet", known: false, window: null };
}

/** The notice under the sentence for the words that need an action. */
function noticeFor(word: string, store: Store): { title: string; body: string; red: boolean; cta?: { label: string; go: () => void } } | null {
  switch (word) {
    case "Update required":
      return {
        title: "Update required",
        body: "This build is for a different genesis than the network it is talking to. Updating keeps your keys, wallet and settings.",
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
        title: "The node cannot start yet",
        body: "The reason is above. The last crash record and the full log are on the Advanced tab.",
        red: true,
        cta: { label: "Open Advanced", go: () => store.go("node/advanced") },
      };
    default:
      return null;
  }
}

/**
 * The validator card: the one grant story (C1-V V5) for a member with a grant, a recorded
 * activation's status with Try again / Forget (#406), or Activate validator when activationOffer
 * allows it (the one gate, on a page that reads Synced). Linking this device's wallet comes first:
 * the bond is paid to the wallet the identity names, and the self-bond is sent from this device.
 * No amount is shown: the minimum stake is read, never hard-coded here.
 */
function ValidatorCard({ store, s, pageWord }: { store: Store; s: AppState; pageWord: string }) {
  const offer = activationOffer(pageWord, s);
  const bond = bondFlags(s);
  const grant = s.hasGrant ? (
    <p data-testid="grant-story" style={{ fontSize: 11.5, color: "var(--tx-3)", margin: 0, lineHeight: 1.5 }}>
      {grantExplanation({ amountSalt: fmtSaltFromWei(s.s5StakeWei), unlockBlock: s.s5UnlockBlock, staked: s.bondedStake > 0, granted: s.hasGrant, activated: bond.activated })}
    </p>
  ) : null;
  // A recorded activation exists only for a member with a grant (S7 / this card sent it).
  const recorded = s.hasGrant && hasActivationTxStatus(store, s);
  let body;
  if (recorded) {
    body = <ActivationTxPanel store={store} s={s} testId="node-activation" />;
  } else if (!offer.offer) {
    body = (
      <span data-testid="activate-gate" style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)", overflowWrap: "anywhere" }}>
        {offer.why}
      </span>
    );
  } else if (!store.walletIsLinked()) {
    body = (
      <>
        <p style={{ fontSize: 12, color: "var(--tx-2)", margin: 0, lineHeight: 1.5 }}>
          Your node is synced. Before activating, link this device&rsquo;s wallet to your Citrate identity: your membership bond is paid to the wallet your identity names, and it must be
          this one or the funds land somewhere this device cannot spend from.
        </p>
        <button className="btn btn-primary btn-sm" style={{ alignSelf: "flex-start" }} onClick={() => void store.linkWallet()}>
          Link this wallet · no funds move
        </button>
        <span style={{ fontSize: 11.5, color: "var(--tx-2)" }}>You&rsquo;ll approve a signature that proves you control this wallet. Nothing is spent.</span>
      </>
    );
  } else {
    body = (
      <>
        <p style={{ fontSize: 12, color: "var(--tx-2)", margin: 0, lineHeight: 1.5 }}>
          Your node is synced and this wallet is linked to your identity. Activating stakes your bond&rsquo;s principal in the ValidatorRegistry; the registry sets the epoch your
          validator becomes active, shown here once registered. You approve it in the Signature Ceremony.
        </p>
        <button className="btn btn-primary btn-sm" style={{ alignSelf: "flex-start" }} onClick={() => void store.requestValidatorActivation()}>
          {activateValidatorLabel(s.activation)}
        </button>
      </>
    );
  }
  return (
    <div className="surface" data-testid="validator-card" style={{ padding: "14px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
      <span className="eyebrow">Validator</span>
      {grant}
      {body}
    </div>
  );
}
