// =====================================================================
// citrate-core: Node › Status words (core plan K3, UI truth contract §1 "Network and node")
//
// One pure function turns what this build reads about the local node into exactly one of the
// contract's node words. Data sources (Rule 7):
//   - supervisor state, local eth_blockNumber, local net_peerCount: `node_status` (node.rs), folded
//     into AppState by Store.refreshNode every 2 s (`s.node`, `s.height`, `s.peers`), committed with
//     the time of that answer (`s.nodeStatusAt`);
//   - why a start was refused: `node_status.blocked` (`s.nodeBlocked`);
//   - public head: eth_blockNumber on the profile's RPC (wagmi useBlockNumber);
//   - genesis check: block 0 of the public chain against the active network profile
//     (networkCompat.ts, K6).
//   - CORE-N4 (core#589): the node's own `citrate_nodeIdentity.syncState` and the head heights its
//     peers announced (`net_peerInfo[].headHeight`), both from `node_status` (`s.nodeSyncState`,
//     `s.peerTips`) and both feature-detected: a node without them (before r1007) gives null.
// "Synced" means this node is within 5 blocks of EVERY network view this build has: the public
// RPC head read in the last 60 s, the peers' quorum tip (the height a strict majority of at least
// two reporting peers are at or above, so one peer far ahead or behind does not decide), and the
// node's own highest-seen height. With no view it is "Sync unknown · network head unavailable"
// (contract v1.1 §1), never Synced or Syncing. A node more than 5 blocks AHEAD of the views is
// "Sync unknown · this node is ahead of the network head" (core#589: a local chain the network
// does not have). A node that reports `syncState: wedged` reads "Wedged", whatever its height.
// Unknown is never zero: a local height of 0 means "RPC not answering yet" (the call returns 0x0
// on error), never block 0.
// =====================================================================
import type { AppState } from "../../shell/state";
import { UNACTIVATED_WITHDRAW_NOTE, canOfferActivation, validatorDash, validatorWord } from "../../shell/chainTruth";
import type { NetworkCompat } from "../../shell/networkCompat";

/** Contract §1: Synced means within this many blocks of the public RPC head. */
export const SYNCED_WITHIN_BLOCKS = 5;
/** Contract §1: No peers means net_peerCount = 0 for at least this long. */
export const NO_PEERS_AFTER_MS = 60_000;
/** Contract §1: a read older than this is stale, so unknown (public head freshness, Sync stalled). */
export const FRESH_MS = 60_000;
/** The contract v1.1 §1 word for a node whose sync can't be checked against the network. */
export const SYNC_UNKNOWN = "Sync unknown · network head unavailable";
/** core#589: a node more than SYNCED_WITHIN_BLOCKS ahead of every network view. */
export const SYNC_AHEAD = "Sync unknown · this node is ahead of the network head";
/** The fewest peers whose announced heights form a view of the network (one peer is not a quorum). */
export const QUORUM_MIN_PEERS = 2;

/** `citrate_nodeIdentity.syncState` as node_status passes it (sync_truth.rs `NodeSyncState`). */
export interface NodeSyncStateView {
  state: string;
  reason: string | null;
  forkDepth: number | null;
  appliedHeight: number | null;
  highestSeen: number | null;
}

/**
 * The peers' view of the network tip: the highest height a strict majority of the reporting peers
 * are at or above, or null with fewer than QUORUM_MIN_PEERS reports. Sorted high to low, that is
 * the entry at index floor(n / 2).
 */
export function quorumTip(tips: readonly number[]): number | null {
  const ok = tips.filter((t) => Number.isSafeInteger(t) && t > 0);
  if (ok.length < QUORUM_MIN_PEERS) return null;
  const sorted = [...ok].sort((a, b) => b - a);
  return sorted[Math.floor(sorted.length / 2)];
}

export type NodeTone = "off" | "busy" | "ok" | "warn" | "bad";

export interface NodeWordInput {
  /** The store's node lifecycle, from the supervisor state (Store.mapNodeState). */
  node: AppState["node"];
  /** `node_status.blocked.message`: another process holds the chain database or a node port. */
  blocked: string | null;
  /** Genesis check of the public chain against this build's network profile (networkCompat.ts). */
  compat: NetworkCompat;
  /** Local eth_blockNumber; 0 = not answered yet. */
  localHeight: number;
  /** Public eth_blockNumber; null = not read. */
  publicHead: number | null;
  /** Age of the last good public head read (ms); null = unknown. */
  publicHeadAgeMs: number | null;
  /** The latest public head poll failed (the last good value may still be held). */
  publicHeadError: boolean;
  /** Local net_peerCount. */
  peers: number;
  /** How long the local peer count has read 0 while the node answers RPC (ms). */
  peersZeroForMs: number;
  /** How long the local height has been unchanged while the node answers RPC (ms). */
  localStillForMs: number;
  /** CORE-N4: `node_status.syncState` (citrate_nodeIdentity); null/absent = the node has none. */
  nodeSyncState?: NodeSyncStateView | null;
  /** CORE-N4: `node_status.peerTips` (net_peerInfo head heights); null/absent = not exposed. */
  peerTips?: number[] | null;
}

export interface NodeWord {
  /** The contract word, e.g. "Synced" or "Syncing · 120 blocks behind". */
  word: string;
  /** Why this word, in plain language, naming the reads behind it. */
  why: string;
  tone: NodeTone;
  /** Whether the node process is up (Start is hidden, Stop shown). */
  running: boolean;
}

const fmt = (n: number) => Math.round(n).toLocaleString("en-US");

/** The contract §1 node word for this node. */
export function nodeWord(i: NodeWordInput): NodeWord {
  const up = i.node !== "off" && i.node !== "error" && i.node !== "paused";
  if (i.compat === "retired") {
    return {
      word: "Update required",
      why: "The public chain's genesis block differs from the genesis this build ships with (eth_getBlockByNumber(0x0).hash), so this build is for a different network.",
      tone: "bad",
      running: up,
    };
  }
  if (i.compat === "unset") {
    // K6: a profile with any unset value refuses node starts and every signature (Rust ready()).
    return {
      word: "Update required",
      why: "This build's network profile is not complete, so it refuses to start a node or sign anything (network/profiles.json). An update ships the profile.",
      tone: "bad",
      running: up,
    };
  }
  if (i.blocked || i.node === "error") {
    return {
      word: "Error",
      why: i.blocked ?? "The node failed to start or crashed. The last crash record and the full log are on the Advanced tab.",
      tone: "bad",
      running: false,
    };
  }
  if (!up) return { word: "Off", why: "The node process isn't running.", tone: "off", running: false };
  if (i.node === "prov" || i.localHeight <= 0) {
    return { word: "Starting", why: "The process started but its RPC isn't answering yet.", tone: "busy", running: true };
  }
  if (i.nodeSyncState?.state === "wedged") {
    const d = i.nodeSyncState.forkDepth;
    return {
      word: d != null && d > 0 ? `Wedged · ${fmt(d)} blocks deep on a dead branch` : "Wedged · on a dead branch",
      why:
        "The node reports syncState wedged (citrate_nodeIdentity): its applied tip is held on a branch the network does not continue, so it stops serving and producing on it" +
        (i.nodeSyncState.reason ? ` (${i.nodeSyncState.reason}).` : ".") +
        " A node built with the guarded cold resync moves that chain data aside and syncs again by itself.",
      tone: "bad",
      running: true,
    };
  }
  if (i.node === "stalled" || (i.peers > 0 && i.localStillForMs >= FRESH_MS)) {
    return {
      word: "Sync stalled",
      why: "Peers are connected, but this node's height hasn't moved for 60 s or more.",
      tone: "warn",
      running: true,
    };
  }
  if (i.peers === 0 && i.peersZeroForMs >= NO_PEERS_AFTER_MS) {
    return {
      word: "No peers",
      why: "This node has had no direct peers for 60 s or more (local net_peerCount), so it can't reach the network.",
      tone: "warn",
      running: true,
    };
  }
  // The network views this build has (CORE-N4), each named for the reason line.
  const views: { label: string; tip: number }[] = [];
  if (headFresh(i) && i.publicHead != null) views.push({ label: "the public RPC head read in the last 60 s", tip: i.publicHead });
  const reporting = (i.peerTips ?? []).filter((t) => Number.isSafeInteger(t) && t > 0).length;
  const peerTip = quorumTip(i.peerTips ?? []);
  if (peerTip != null) views.push({ label: `the height a majority of ${reporting} peers announced`, tip: peerTip });
  const seen = i.nodeSyncState?.highestSeen;
  if (seen != null && seen > 0) views.push({ label: "the highest network height this node has seen", tip: seen });
  if (views.length === 0) {
    return {
      word: SYNC_UNKNOWN,
      why: "The public RPC head couldn't be read in the last 60 s (error, timeout, 0x0 or not read yet) and no quorum of peers announced a height, so this node's height can't be checked against the network.",
      tone: "busy",
      running: true,
    };
  }
  const top = Math.max(...views.map((v) => v.tip));
  const named = views.map((v) => v.label).join(", ");
  if (i.localHeight - top > SYNCED_WITHIN_BLOCKS) {
    return {
      word: SYNC_AHEAD,
      why: `This node's height is ${fmt(i.localHeight - top)} blocks above every network view (${named}), so it may be on a chain the network does not have; it is not counted as synced.`,
      tone: "warn",
      running: true,
    };
  }
  const behind = top - i.localHeight;
  if (behind <= SYNCED_WITHIN_BLOCKS) {
    return {
      word: "Synced",
      why: `This node's height is within ${SYNCED_WITHIN_BLOCKS} blocks of every network view this build has: ${named}.`,
      tone: "ok",
      running: true,
    };
  }
  return {
    word: `Syncing · ${fmt(behind)} blocks behind`,
    why: "Applying blocks to catch up: the public head minus this node's height. No percentage is shown.",
    tone: "busy",
    running: true,
  };
}

/** The public head is usable: read, not 0x0, the last poll answered, and no older than 60 s. */
export function headFresh(i: Pick<NodeWordInput, "publicHead" | "publicHeadAgeMs" | "publicHeadError">): boolean {
  return i.publicHead != null && i.publicHead > 0 && !i.publicHeadError && i.publicHeadAgeMs != null && i.publicHeadAgeMs <= FRESH_MS;
}

/** The one-sentence status: the node word, then the validator word when it has been read. */
export function statusSentence(node: NodeWord, vWord: string | null): string {
  return vWord ? node.word + " · " + vWord : node.word;
}

/** "N direct peers" from the local count, or unknown while the node isn't answering. */
export function directPeers(running: boolean, localHeight: number, peers: number): string {
  if (!running || localHeight <= 0) return "— · not read";
  return peers === 1 ? "1 direct peer" : fmt(peers) + " direct peers";
}

/** Local height against the public head, as numbers that were actually read. */
export function heightLine(localHeight: number, publicHead: number | null): string {
  if (localHeight <= 0) return "—";
  if (publicHead == null || publicHead <= 0) return fmt(localHeight) + " (this node) · network head unavailable";
  const behind = publicHead - localHeight;
  return fmt(localHeight) + (behind > 0 ? " · " + fmt(behind) + " behind the public head" : " · at the public head");
}

/** The validator-state reads the Node page decides with (the store's, C1-V). */
export type ValidatorReads = Pick<AppState, "node" | "bondedStake" | "validatorActivation" | "hasGrant" | "bondActivated" | "bondUnlocked" | "activation">;

/**
 * The bond's activated() / isUnlocked() as the store holds them: the grant read, or since #408 the
 * activation read (refreshActivation). Either read counts; an unread one is null.
 */
export function bondFlags(s: Pick<AppState, "bondActivated" | "bondUnlocked" | "activation">): { activated: boolean | null; unlocked: boolean | null } {
  return { activated: s.bondActivated ?? s.activation?.bondActivated ?? null, unlocked: s.bondUnlocked ?? s.activation?.bondUnlocked ?? null };
}

/**
 * Whether Node › Status offers Activate validator, and why not (contract §1 Validators). The
 * decision is the app's one activation gate, `canOfferActivation` (registry status and actEpoch,
 * registered stake, MemberBond.activated() and the bond lock MemberBond.isUnlocked(), all read by
 * the store), and nothing page-local. On top of it the page must read "Synced" against a fresh
 * public head (block production stays unarmed under Sync unknown) and the supervisor must report
 * synced, because arming re-checks the tip in Rust. The reason is the gate's own: the validator
 * word, or "— · <reason>" while a read is missing. store.requestValidatorActivation, the only
 * activation entry, re-runs the full gate on a fresh read before anything is signed (#405, #554).
 */
export function activationOffer(pageWord: string, s: ValidatorReads): { offer: boolean; why: string } {
  const b = bondFlags(s);
  if (canOfferActivation(s.bondedStake, s.validatorActivation, b.activated, b.unlocked)) {
    if (pageWord !== "Synced") return { offer: false, why: "Activation opens once this node reads Synced. It reads " + pageWord + " now." };
    if (s.node !== "synced") return { offer: false, why: "Activation opens once node_status reports this node synced, so block production can be armed." };
    return { offer: true, why: "" };
  }
  const word = validatorWord(s.bondedStake, s.validatorActivation, s.hasGrant, b.activated, b.unlocked);
  switch (word) {
    case "Unlocked · not activated":
      // Contract v1.2 §1: the word and its required detail only; no activation-window copy.
      return { offer: false, why: "Unlocked · not activated · " + UNACTIVATED_WITHDRAW_NOTE };
    case "Not a validator":
      return { offer: false, why: "Not a validator · activation needs the membership grant's bond on chain." };
    case "Exiting":
      return { offer: false, why: "Exiting · this key is leaving the validator registry (validatorInfo.status = 2), and a bond activates only once." };
    case "Slashed":
      return { offer: false, why: "Slashed · a slashed key can't register again (validatorInfo.status = 3)." };
    case null:
      return { offer: false, why: validatorDash(s.bondedStake, s.validatorActivation, s.hasGrant, b.activated, b.unlocked) + ". Activation is offered only once the registry and bond reads say it can be." };
    default:
      return { offer: false, why: word + " · this key already has registered stake (ValidatorRegistry.stakeOf), so there is nothing to activate." };
  }
}

/** Dot colour for a tone (the 0.5.1 node palette). */
export const TONE_COLOR: Record<NodeTone, string> = {
  off: "var(--tx-3)",
  busy: "#ffbd10",
  ok: "#8ecc09",
  warn: "#e8843c",
  bad: "#dd7259",
};
