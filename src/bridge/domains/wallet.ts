// citrate-core: bridge domain `wallet`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

import type { CeremonyView } from "../types";

/** C2-F-1 — the outcome of pressing Claim. Either a REAL pending ceremony the
 * human must approve via `signing.broadcast` (B1.4), or an honest "nothing to
 * claim" when the on-chain claimable is 0. NEVER a faked settlement. */
export type ClaimResult =
  | { kind: "ceremony"; view: CeremonyView }
  | { kind: "nothing"; claimableWei: string };

export interface WalletDomain {
  /** Native liquid SALT (eth_getBalance) and claimable (ContributionAccounting). No self-stake:
   * staking is through the validator bond (D-9 retired the liquid staking pool). */
  balances(): Promise<{ liquid: number; claimable: number; address: string }>;
  /** Q-E.2 (C-5) — the indexed tx history. `status` (1 ok / 0 reverted / null
   * pending) + `direction` are passed through from the indexer so a failed tx can
   * be marked (they used to be stripped, making a revert look like a success). */
  activity(): Promise<{ id: string; kind: string; amount: string; hash: string; ts: number; status?: number | null; direction?: string }[]>;
  /** CORE (@rule8) — submit a native SALT transfer as a PENDING ceremony and
   * return the decoded view. Signs NOTHING; the human approves via
   * `signing.broadcast(view.id)` (B1.4 → a real 40204 tx). `amountWei` is a
   * decimal wei string. Mirrors `agent.claim` for the claimRewards() path. */
  send(to: string, amountWei: string): Promise<CeremonyView>;
  /**
   * Seamlessly device-provision the custody vault and ensure the wallet exists,
   * returning its PUBLIC address (never key/seed). Idempotent — safe to call on
   * every launch; an already-provisioned device short-circuits to a cheap read.
   *
   * WHY IT MATTERS: this is the missing runtime step behind the "Wallet link
   * unavailable" dead-end. On a fresh install the custody vault was uninitialized
   * + locked and no wallet had been minted, so `linkRequest`/`balances`/`address`
   * all failed closed. `ensureReady` init+unlocks the vault under a device-bound
   * keyring passphrase (no user passphrase — the owner-approved model) and mints
   * the wallet silently, so there is a REAL EOA to link and fund. Call it after
   * sign-in and before the membership grant.
   *
   * Tauri → `wallet_ensure_ready` (real custody + wallet). The sim shim returns a
   * deterministic, clearly-labelled NON-CHAIN persona address (no vault/key in the
   * web preview).
   */
  ensureReady(): Promise<{ address: string; created: boolean }>;
  /**
   * A REAL device fingerprint: sha256 over the device-bound custody PUBLIC key.
   * Stable per install, non-secret, and NOT attestation (see provisioning.rs).
   */
  deviceId(): Promise<string>;
  /**
   * Bind THIS device's custody EOA to the member's Citrate identity, as a PENDING
   * ceremony. Signs NOTHING — the human approves via `wallet.linkApprove`.
   *
   * WHY IT MATTERS: until a wallet is linked, the authority's `wallet_address`
   * claim is the counterfactual smart-wallet address, which no private key can
   * spend from. The membership money path pays THAT address, while the validator
   * self-bond is sent from this custody EOA — so an unlinked member is funded
   * somewhere they cannot reach. Linking makes the two the same address.
   *
   * The proof is an EIP-191 signature over a one-time challenge from the
   * authority; the message is shown verbatim in the approval UI.
   */
  linkRequest(): Promise<CeremonyView>;
  /**
   * Approve a pending link ceremony: signs the challenge and submits the proof to
   * the authority. Returns the now-bound address. NEVER returns the signature —
   * it is consumed in-process (I-2). This is NOT `signing.broadcast`: no
   * transaction is sent and no funds move.
   */
  linkApprove(id: string, rawAck: boolean): Promise<{ address: string; linked: boolean; canonical: boolean }>;
  /** Decline a pending link: release the ceremony + drop the one-time nonce. */
  linkReject(id: string): Promise<void>;
}
