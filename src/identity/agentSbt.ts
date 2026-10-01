// =====================================================================
// citrate-core — Hermes identity (AgentSBT), HUP-S7.4 (US-7.1)
//
// Core (src-tauri/src/agent_sbt.rs) owns every decision: it reads the address book, the
// contract code, the member's AgentSBT balance and tokens, the parent organization, and an
// eth_call preflight of the exact mint, and answers with one state plus the sentence to show.
// This file is the wire type, a pure card model, and two thin calls. The card offers the
// mint ONLY when core says `available` (fail closed). The mint itself is a pending
// SignatureCeremony that the member approves (HIC-1); nothing here signs.
// =====================================================================
import { createSlice } from "../shell/slices/createSlice";
import type { CeremonyView } from "../bridge/types";

export type AgentSbtState =
  | "ready"
  | "minted"
  | "not-in-book"
  | "no-code"
  | "org-not-active"
  | "not-issuer"
  | "identity-key-missing"
  | "chain-unreachable"
  | "reverted";

export interface AgentToken {
  tokenId: string;
  parentOrgId: string;
  /** The stored bytes32 DID (keccak of the DID string). */
  did: string;
  pubkeyFingerprint: string;
  quarantined: boolean;
}

/** Mirrors Rust `AgentSbtStatus` (camelCase). */
export interface AgentSbtStatus {
  contract: string | null;
  member: string;
  did: string | null;
  parentOrgId: string;
  balance: string | null;
  /** null = could not be listed (never a guessed empty list). */
  tokens: AgentToken[] | null;
  tokensNote: string | null;
  state: AgentSbtState;
  available: boolean;
  message: string;
}

export interface IdentityCard {
  tone: "checking" | "waiting" | "ready" | "done" | "error";
  title: string;
  body: string;
  button: string;
  canMint: boolean;
  tokens: { label: string; detail: string; quarantined: boolean }[];
  note: string | null;
}

const TITLE = "Hermes identity";
const BUTTON = "Give Hermes an identity";

const shortHex = (h: string) => (h.length > 14 ? `${h.slice(0, 10)}…${h.slice(-4)}` : h);

/** The card for a status. `mode` is the bridge mode; in the web preview there is no core. */
export function identityCard(
  status: AgentSbtStatus | null,
  ctx: { mode: "sim" | "tauri"; loaded: boolean; error: string | null },
): IdentityCard {
  const card = (tone: IdentityCard["tone"], body: string, extra: Partial<IdentityCard> = {}): IdentityCard => ({
    tone,
    title: TITLE,
    body,
    button: BUTTON,
    canMint: false,
    tokens: [],
    note: null,
    ...extra,
  });
  if (ctx.mode !== "tauri") return card("waiting", "Hermes identity is set up in the desktop app.");
  if (!status) {
    if (!ctx.loaded) return card("checking", "Checking Hermes identity on chain 40204…");
    return card("error", ctx.error ? `Could not check Hermes identity: ${ctx.error}` : "Could not check Hermes identity.");
  }
  const tokens = (status.tokens ?? []).map((t) => ({
    label: `AgentSBT #${t.tokenId}`,
    detail: `parent org #${t.parentOrgId} · did ${shortHex(t.did)}`,
    quarantined: t.quarantined,
  }));
  const note = status.tokens === null ? status.tokensNote : null;
  if (status.state === "minted") return card("done", status.message, { tokens, note });
  if (status.state === "ready" && status.available) return card("ready", status.message, { canMint: true });
  if (status.state === "chain-unreachable" || status.state === "reverted") return card("error", status.message);
  return card("waiting", status.message);
}

// ---------------------------------------------------------------- slice + calls

export interface AgentSbtSliceState {
  status: AgentSbtStatus | null;
  loaded: boolean;
  error: string | null;
  /** A mint request is being prepared. */
  busy: boolean;
}

export const agentSbtSlice = createSlice<AgentSbtSliceState>({ status: null, loaded: false, error: null, busy: false });

/** The core seam: the bridge mode and the timeout-wrapped invoke. Injected so tests need no Tauri. */
export interface AgentSbtIo {
  mode: "sim" | "tauri";
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
}

const message = (e: unknown): string => (e instanceof Error ? e.message : typeof e === "string" ? e : String(e));

/** The production seam. */
export async function defaultAgentSbtIo(): Promise<AgentSbtIo> {
  const { BRIDGE_MODE } = await import("../bridge/mode");
  const { invoke } = await import("../bridge/tauri/invoke");
  return { mode: BRIDGE_MODE, invoke: (cmd, args) => invoke(cmd, args) };
}

/** Read the status from core into the slice. The web preview makes no call. */
export async function loadAgentSbt(io: AgentSbtIo): Promise<void> {
  if (io.mode !== "tauri") {
    agentSbtSlice.set({ status: null, loaded: true, error: null });
    return;
  }
  try {
    const status = await io.invoke<AgentSbtStatus>("agent_sbt_status");
    agentSbtSlice.set({ status, loaded: true, error: null });
  } catch (e) {
    agentSbtSlice.set({ status: null, loaded: true, error: message(e) });
  }
}

/** Ask core for the pending mint ceremony. Core re-checks readiness and refuses with the
 *  member-facing reason; that reason is rethrown as-is. */
export async function requestAgentSbtMint(io: AgentSbtIo): Promise<CeremonyView> {
  if (io.mode !== "tauri") throw new Error("Hermes identity is set up in the desktop app.");
  try {
    return await io.invoke<CeremonyView>("agent_sbt_mint");
  } catch (e) {
    throw new Error(message(e));
  }
}
