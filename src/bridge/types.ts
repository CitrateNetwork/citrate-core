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
