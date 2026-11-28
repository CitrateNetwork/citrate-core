// citrate-core: bridge domain `social`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

import type { CeremonyView } from "../types";

// ── Social identity (Connections · social discovery). Privacy model: ADR-2026-08-30. ──
// The address↔identity binding is device-local + shared server-blind; NEVER on-chain by default;
// private by default; "verified" requires OAuth ownership + a wallet-signed IdentityBinding through
// the Signature Ceremony (Rule 3). The OAuth token seals in the OS keyring and NEVER crosses this
// bridge — only the LinkedIdentity connect facts do.
export type SocialNetwork = "x" | "linkedin" | "discord";
export type SocialVisibility = "private" | "groups";
export interface LinkedIdentity {
  network: SocialNetwork;
  /** The display handle from the verified OAuth account (e.g. "dana"). */
  handle: string;
  /** verified = OAuth ownership + wallet-signed IdentityBinding via the ceremony (ADR D3). */
  verified: boolean;
  /** private (default) | groups-only. Never public unless the explicit on-chain opt-in is taken (ADR D2). */
  visibility: SocialVisibility;
  linkedAt: number;
  /** #61 — published to the opt-in find-via-X directory (a separate, more public opt-in than visibility). */
  directoryPublished: boolean;
}

/** #61 — a live directory binding for an exact handle (find-via-X lookup). Mirrors the Rust DirectoryHit. */
export interface DirectoryHit {
  address: string;
  boundAt: number;
}

/** #61 — one directory search (typeahead) row. Mirrors the Rust DirectorySearchHit. */
export interface DirectorySearchHit {
  handle: string;
  address: string;
  displayName?: string;
}
/** A face a viewer may see for an address — a verified, group-visible handle (resolver output). */
export interface ResolvedIdentity {
  address: string;
  network: SocialNetwork;
  handle: string;
}
/** The shareable binding payload that rides a group message (server-blind). Carries no token. */
export interface ExportedBinding {
  network: SocialNetwork;
  handle: string;
  address: string;
  nonce: string;
  signature: string;
}
/** Control-message sentinel: a group message body starting with this carries an ExportedBinding
 *  (JSON follows). Clients ingest + hide these; they never render in the conversation. */
export const SOCIAL_BINDING_MSG_PREFIX = "cbind1:";
export interface SocialDomain {
  /** The user's current linked identities (device-local). Honest-empty when none/unwired. */
  status(): Promise<LinkedIdentity[]>;
  /** Begin the OAuth ownership proof (opens the system browser, loopback-PKCE). Returns the resulting
   *  UNVERIFIED, private link (handle from OAuth). The token seals in the keyring, never returned. */
  start(network: SocialNetwork): Promise<LinkedIdentity>;
  /** Open a ceremony over the wallet-signed IdentityBinding (ADR D3). Returns the CeremonyView the
   *  approval UI renders; signs NOTHING (the wallet signs at approve). */
  verifyRequest(network: SocialNetwork): Promise<CeremonyView>;
  /** Approve a SPECIFIC pending verification id → the wallet signs the binding at the ceremony and it
   *  is recorded; the link becomes verified. Never returns a signature. */
  verifyApprove(id: string, rawAck: boolean): Promise<LinkedIdentity>;
  /** Drop a pending verification the human rejected. */
  verifyForget(id: string): Promise<void>;
  /** Set a link's visibility (private | groups). Narrowing takes effect immediately (ADR D2). */
  setVisibility(network: SocialNetwork, visibility: SocialVisibility): Promise<LinkedIdentity>;
  /** Forget a link — drop the local record + tombstone to group members (ADR revocation). */
  disconnect(network: SocialNetwork): Promise<void>;
  /** Resolve member addresses → verified, group-visible faces this device knows (own + foreign). */
  resolve(addresses: string[]): Promise<ResolvedIdentity[]>;
  /** The shareable payload for a verified, group-visible link (null if none) — rides a group message. */
  exportBinding(network: SocialNetwork): Promise<ExportedBinding | null>;
  /** Accept a peer's binding from the relay. VERIFIES (sender==address + signature recovers) before
   *  storing; returns whether it was accepted. */
  ingestBinding(sender: string, binding: ExportedBinding): Promise<boolean>;
  /** #61 — opt-in PUBLISH a verified link to the find-via-X directory. Opens a ceremony over the
   *  directory-scoped statement the wallet signs (ADR D-7 exception; self-published only). Returns the
   *  CeremonyView; signs NOTHING (approve at directoryPublishApprove). Requires a verified link. */
  directoryPublishRequest(network: SocialNetwork): Promise<CeremonyView>;
  /** #61 — approve a pending publish → the wallet signs, the binding POSTs to the authority with the
   *  member's Bearer, and the link is marked published. Returns the updated link. Never a signature. */
  directoryPublishApprove(id: string, rawAck: boolean): Promise<LinkedIdentity>;
  /** #61 — open a ceremony to REVOKE (unpublish) a directory binding. Returns the CeremonyView. */
  directoryUnpublishRequest(network: SocialNetwork): Promise<CeremonyView>;
  /** #61 — approve a pending revoke → sign, tombstone at the authority, mark unpublished. */
  directoryUnpublishApprove(id: string, rawAck: boolean): Promise<LinkedIdentity>;
  /** #61 — drop a pending directory ceremony the human rejected. */
  directoryForget(id: string): Promise<void>;
  /** #61 — resolve an EXACT social handle to its published address, or null if nobody opted in. */
  directoryLookup(platform: SocialNetwork, handle: string): Promise<DirectoryHit | null>;
  /** #61 — typeahead over published handles (find-via-X). Honest-empty on no match. */
  directorySearch(platform: SocialNetwork, query: string): Promise<DirectorySearchHit[]>;
}
