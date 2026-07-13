// =====================================================================
// citrate-core — bridge shared types (CORE-A1)
//
// The bridge is the ONE seam between the surfaces and the backend. It is
// runtime-selected between `sim` (dev/web, the current prototype behind a
// shim) and `tauri` (the packaged app, invoking Rust commands). Every later
// phase flips one domain's implementation sim→live; the surfaces never change.
//
// Rule 1 (no mocks-as-live): on the Tauri path, a domain that is not yet wired
// returns `Unavailable` — an honest seam state — never fabricated data. The sim
// path is a clearly-namespaced dev shim, guarded out of packaged builds.
// =====================================================================

export type BridgeMode = "sim" | "tauri";

/**
 * Honest "not wired yet" signal. On the Tauri path an unwired domain throws
 * this; the surface maps it to its existing "coming"/seam copy. This is the
 * Rule-1 line: the real desktop app says a domain is unavailable rather than
 * showing sim data dressed as live.
 */
export class Unavailable extends Error {
  readonly kind = "unavailable" as const;
  readonly domain: string;
  readonly op: string;
  constructor(domain: string, op: string, detail?: string) {
    super(
      `bridge domain "${domain}" op "${op}" is unavailable in this build` +
        (detail ? ` — ${detail}` : ""),
    );
    this.name = "Unavailable";
    this.domain = domain;
    this.op = op;
  }
}

export function isUnavailable(e: unknown): e is Unavailable {
  return e instanceof Unavailable || (typeof e === "object" && e !== null && (e as { kind?: string }).kind === "unavailable");
}

/**
 * The persisted, genuinely-real app config (A1.4). This is the one domain wired
 * end-to-end through a real on-disk Tauri store. Shapes mirror the AppState
 * fields the Settings surface already reads/writes.
 */
export interface AppConfig {
  net: "testnet" | "local";
  rpc: "local" | "public";
  dataDir: string;
  cpuCap: number;
  autolock: number;
  channel: "stable" | "beta";
  telemetry: boolean;
  sigPolicy: "hitl" | "allow";
}

export const DEFAULT_APP_CONFIG: AppConfig = {
  net: "testnet",
  rpc: "local",
  dataDir: "~/.citrate/core",
  cpuCap: 50,
  autolock: 30,
  channel: "stable",
  telemetry: false,
  sigPolicy: "hitl",
};

export type KeyringStatus = "available" | "unavailable" | "unknown";

/**
 * Custody vault status crossing the bridge (CORE-A2). Metadata ONLY — the
 * bridge custody domain NEVER returns secret bytes to the frontend (ADV-8).
 * Reading a secret is an in-process Rust API (`custody_get`), not an invoke.
 */
export interface CustodyStatus {
  /** An on-disk envelope exists (the vault has been initialized). */
  initialized: boolean;
  /** A session is currently unlocked (respecting auto-lock). */
  unlocked: boolean;
  /** Auto-lock window in minutes (mirrors `config.autolock`). */
  autolockMins: number;
  /** OS keyring availability, read from the platform (not fabricated). */
  keyringStatus: KeyringStatus;
}

/** Slot metadata crossing the bridge — name + ciphertext length, never bytes. */
export interface SlotInfo {
  name: string;
  bytes: number;
}

/**
 * CORE-A3 — the claim-derived auth status crossing the bridge. This is the ONLY
 * thing the auth commands return: flags + entitlement claims, NEVER a token
 * (ADV-8). The rotating refresh token lives in the A2 custody vault; the access
 * token lives only in Rust memory. Neither ever crosses this boundary.
 */
export interface AuthStatus {
  /** A live session exists (an access token is held in the Rust process). */
  signedIn: boolean;
  sub: string | null;
  tier: string | null;
  org: string | null;
  role: string | null;
  /** KYC claim: none | pending | verified | failed | review (S2 seam). */
  kycStatus: string | null;
  /** Smart-wallet address claim (display only — A3 does NOT sign; rule 3). */
  walletAddr: string | null;
  /** Entitlement expiry (from the claim), for the Settings/onboarding UI. */
  expiresAt: string | null;
  email: string | null;
}

/**
 * CORE-B1.2 — the SignatureCeremony bridge types. The `signing` domain is the
 * ONE human-in-the-loop signing path: a caller SUBMITS an intent (never signs),
 * a human APPROVES a specific ceremony id, and only then a signature is produced.
 * NONE of these types carries key/seed/entropy material (I-2): a request returns
 * a decoded VIEW, an approve returns a SIGNATURE (hex), never a key.
 */
export type IntentKind = "personal_sign" | "typed_data" | "transaction";

/** A signature intent submitted to the ceremony. `origin` is displayed verbatim
 * (never trusted to be benign); `raw` is the hex payload to sign. */
export interface SignatureIntent {
  /** Who is asking (origin URL / "local-user" / agent id). Displayed verbatim. */
  origin: string;
  kind: IntentKind;
  /** Chain id the intent targets (40204 for Citrate). */
  chainId: number;
  /** Hex payload to sign (message bytes / typed-data JSON / tx bytes). */
  raw: string;
}

/** The decoded, human-readable action surfaced for approval. `action` is
 * `"Unrecognized"` when the calldata cannot be decoded (raw-ack gated). */
export interface DecodedAction {
  action: string;
  cost: string;
  destination: string;
}

/** The pending ceremony view returned by `request` — id + TRUE origin + decoded
 * action. Carries NO signature and NO key material (safe across the bridge). */
export interface CeremonyView {
  /** The single-use id the human must approve/reject EXPLICITLY (no "latest"). */
  id: string;
  /** The TRUE origin, displayed verbatim (anti-spoof). */
  origin: string;
  kind: IntentKind;
  chainId: number;
  decoded: DecodedAction;
  /** Approval is BLOCKED until an explicit raw-mode ack (undecodable calldata). */
  requiresRawAck: boolean;
}

/** A signature result — hex `r||s`, NEVER key material. */
export interface Signature {
  sigHex: string;
  kind: IntentKind;
}

/** The Unrecognized action marker (undecodable calldata → raw-ack gated). */
export const UNRECOGNIZED_ACTION = "Unrecognized";

export const SIGNED_OUT_AUTH: AuthStatus = {
  signedIn: false,
  sub: null,
  tier: null,
  org: null,
  role: null,
  kycStatus: null,
  walletAddr: null,
  expiresAt: null,
  email: null,
};
