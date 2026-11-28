// citrate-core: bridge domain `custody`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

import type { CustodyStatus, SlotInfo } from "../types";

// ---- custody (A2 — the OS-keyring vault) ----------------------------
// Status + slot METADATA only. This domain NEVER returns secret bytes to the
// frontend (ADV-8); reading a secret is an in-process Rust API, not an invoke.
export interface CustodyDomain {
  /** Vault status: initialized / unlocked / auto-lock / keyring (real in Tauri). */
  status(): Promise<CustodyStatus>;
  /** Initialize a fresh vault from a passphrase (mints the keyring master key). */
  init(passphrase: string): Promise<void>;
  /** Unlock the session with the passphrase. */
  unlock(passphrase: string): Promise<void>;
  /**
   * Seamless device-bound RE-UNLOCK (passphrase-less model). Re-provisions +
   * unlocks the vault from the OS-keyring device passphrase — no user passphrase.
   * Idempotent; safe to call on launch/resume and from the Settings "Unlock"
   * control. Returns the fresh status. This is what keeps an auto-locked vault (and
   * the wallet reads gated on it) from getting permanently stuck.
   */
  ensureUnlocked(): Promise<CustodyStatus>;
  /** Lock the session (drops + zeroizes the in-memory data key). */
  lock(): Promise<void>;
  /** Slot metadata only — never secret bytes. */
  listSlots(): Promise<SlotInfo[]>;
}
