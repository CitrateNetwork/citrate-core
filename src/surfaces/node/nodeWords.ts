// =====================================================================
// citrate-core — Node › Status words (core plan K3, UI truth contract §1 "Network and node")
//
// One pure function turns what this build can read about the local node into exactly one of the
// contract's node words. Data sources (Rule 7):
//   - supervisor state, local eth_blockNumber, local net_peerCount: `node_status` (node.rs), folded
//     into AppState by Store.refreshNode every 2 s (`s.node`, `s.height`, `s.peers`);
//   - why a start was refused: `node_status.blocked` (`s.nodeBlocked`);
//   - public head: eth_blockNumber on rpc.citrate.ai (wagmi useBlockNumber);
//   - genesis check: block 0 of the public chain against the bundled address book's genesis
//     (networkCompat.ts).
// `eth_syncing` is not read by any command yet, so "Synced" uses the contract's "within 5 blocks of
// the public RPC head" half and says so in its reason. Unknown is never zero: a local height of 0
// means "RPC not answering yet" (the call returns 0x0 on error), never block 0.
// =====================================================================
import type { AppState } from "../../shell/state";
import type { NetworkCompat } from "../../shell/networkCompat";

/** Contract §1: Synced means within this many blocks of the public RPC head. */
export const SYNCED_WITHIN_BLOCKS = 5;
/** Contract §1: No peers means net_peerCount = 0 for at least this long. */
export const NO_PEERS_AFTER_MS = 60_000;

export type NodeTone = "off" | "busy" | "ok" | "warn" | "bad";

export interface NodeWordInput {
  /** The store's node lifecycle, from the supervisor state (Store.mapNodeState). */
  node: AppState["node"];
  /** `node_status.blocked.message`: another process holds the chain database or a node port. */
  blocked: string | null;
  /** Genesis check of the public chain against this build (networkCompat.ts). */
  compat: NetworkCompat;
  /** Local eth_blockNumber; 0 = not answered yet. */
  localHeight: number;
  /** Public eth_blockNumber; null = not read. */
  publicHead: number | null;
  /** Local net_peerCount. */
  peers: number;
  /** How long the local peer count has read 0 while the node answers RPC (ms). */
  peersZeroForMs: number;
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
  if (i.compat === "retired") {
    return {
      word: "Update required",
      why: "The public chain's genesis block differs from the genesis this build ships with (eth_getBlockByNumber(0x0).hash), so this build is for a different network.",
      tone: "bad",
      running: i.node !== "off" && i.node !== "error",
    };
  }
  if (i.blocked || i.node === "error") {
    return {
      word: "Error",
      why: i.blocked ?? "The node failed to start or crashed. Crash records and the full log are on the Advanced tab.",
      tone: "bad",
      running: false,
    };
  }
  if (i.node === "off" || i.node === "paused") {
    return { word: "Off", why: "The node process isn't running.", tone: "off", running: false };
  }
  if (i.node === "prov" || i.localHeight <= 0) {
    return { word: "Starting", why: "The process started but its RPC isn't answering yet.", tone: "busy", running: true };
  }
  if (i.node === "stalled") {
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
  if (i.publicHead == null || i.publicHead <= 0) {
    return {
      word: "Syncing · — blocks behind",
      why: "The public head hasn't been read yet, so how far behind this node is is unknown.",
      tone: "busy",
      running: true,
    };
  }
  const behind = i.publicHead - i.localHeight;
  if (behind <= SYNCED_WITHIN_BLOCKS) {
    return {
      word: "Synced",
      why: `This node's height is within ${SYNCED_WITHIN_BLOCKS} blocks of the public RPC head. eth_syncing is not read by this build yet.`,
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

/** The one-sentence status: the node word, then the validator word when it has been read. */
export function statusSentence(node: NodeWord, validatorWord: string | null): string {
  return validatorWord ? node.word + " · " + validatorWord : node.word;
}

/** "N direct peers" from the local count, or unknown while the node isn't answering. */
export function directPeers(running: boolean, localHeight: number, peers: number): string {
  if (!running || localHeight <= 0) return "— · not read";
  return peers === 1 ? "1 direct peer" : fmt(peers) + " direct peers";
}

/** Local height against the public head, as numbers that were actually read. */
export function heightLine(localHeight: number, publicHead: number | null): string {
  if (localHeight <= 0) return "—";
  if (publicHead == null || publicHead <= 0) return fmt(localHeight) + " · public head not read";
  const behind = publicHead - localHeight;
  return fmt(localHeight) + (behind > 0 ? " · " + fmt(behind) + " behind the public head" : " · at the public head");
}

/** Dot colour for a tone (the 0.5.1 node palette). */
export const TONE_COLOR: Record<NodeTone, string> = {
  off: "var(--tx-3)",
  busy: "#ffbd10",
  ok: "#8ecc09",
  warn: "#e8843c",
  bad: "#dd7259",
};
