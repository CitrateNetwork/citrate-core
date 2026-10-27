// K3 red-first stub (replaced by the Node › Status words in the next commit). It keeps the old
// behaviour: the store's node label is the word, and the activation offer is the old gate alone.
import type { AppState } from "../../shell/state";
import { nodeLabel } from "../../shell/state";
import { canOfferActivation } from "../../shell/chainTruth";
import type { NetworkCompat } from "../../shell/networkCompat";

export interface NodeWordInput {
  node: AppState["node"];
  blocked: string | null;
  compat: NetworkCompat;
  localHeight: number;
  publicHead: number | null;
  publicHeadAgeMs: number | null;
  publicHeadError: boolean;
  peers: number;
  peersZeroForMs: number;
  localStillForMs: number;
}
export interface NodeWord {
  word: string;
  why: string;
  tone: "off" | "busy" | "ok" | "warn" | "bad";
  running: boolean;
}
export function nodeWord(i: NodeWordInput): NodeWord {
  return { word: nodeLabel(i.node), why: "", tone: "ok", running: i.node !== "off" };
}
export function statusSentence(node: NodeWord): string {
  return node.word;
}
export function directPeers(_running: boolean, _localHeight: number, peers: number): string {
  return String(peers);
}
export function heightLine(localHeight: number): string {
  return String(localHeight);
}
export type ValidatorReads = Pick<AppState, "node" | "bondedStake" | "validatorActivation" | "hasGrant" | "bondActivated" | "bondUnlocked" | "activation">;
export function activationOffer(_pageWord: string, s: ValidatorReads): { offer: boolean; why: string } {
  return { offer: canOfferActivation(s.bondedStake, s.validatorActivation, s.bondActivated, s.bondUnlocked), why: "" };
}
