// citrate-core: bridge domain `signing`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

import type { BroadcastResult, CeremonyView, Signature, SignatureIntent } from "../types";

// ---- signing (B1.2 — the ONE human-in-the-loop signing path) --------
// The gated Rust signer is reachable ONLY through this domain's approve. A
// caller (user / agent / micro-app) SUBMITS an intent; a human APPROVES a
// specific id; only then is a signature produced. No method returns key/seed/
// entropy material (I-2) — request returns a decoded view, approve a signature.
export interface SigningDomain {
  /** Submit an intent → a PENDING ceremony (id + decoded action). Signs NOTHING. */
  request(intent: SignatureIntent): Promise<CeremonyView>;
  /** Approve a SPECIFIC pending id (no "approve latest"/auto-approve). `rawAck`
   * MUST be true to approve undecodable calldata. Returns the signature hex. */
  approve(id: string, rawAck: boolean): Promise<Signature>;
  /** CORE-B1.4 — approve a SPECIFIC pending TRANSACTION id, sign the real EIP-155
   * tx with the vault key, broadcast it to the live 40204 RPC, and return the
   * real tx hash + block (never key material). Same explicit-id / raw-ack /
   * single-use / fail-closed invariants as `approve`. */
  broadcast(id: string, rawAck: boolean): Promise<BroadcastResult>;
  /**
   * Join-8 — the same approval as `broadcast`, split in two so the caller learns the tx hash
   * BEFORE anything is sent: `prepare` approves + signs (sends nothing) and returns the hash
   * (keccak256 of the signed raw); `sendPrepared` sends that signed tx once and polls its receipt.
   * One approval, one signature, at most one send. Tauri → `sign_prepare` / `sign_send_prepared`.
   */
  prepare(id: string, rawAck: boolean): Promise<{ txHash: string }>;
  sendPrepared(id: string): Promise<BroadcastResult>;
  /** Reject a pending ceremony — consumes it, produces no signature. */
  reject(id: string): Promise<void>;
}
