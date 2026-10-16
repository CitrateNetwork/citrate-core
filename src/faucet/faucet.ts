// HUP-S6.5: the in-app faucet for deploy gas: types, pure helpers and the bridge to core.
//
// Faucet ADR (ADR-2026-10-01-faucet-for-deploy-gas) is PROPOSED; owner questions O-1..O-4 are
// open. The in-app faucet is OFF until the member turns it on in Settings → Budgets, which is
// the member's HIC-1 grant of an HIC-2 budget: one top-up per 24 hours, for their own wallet,
// only when a deploy they started is short of gas. Core owns every decision and the request
// itself; this file only reads state and calls core's commands. Nothing here signs anything.
import { BRIDGE_MODE } from "../bridge/mode";
import { invoke } from "../bridge/tauri/invoke";

export type FaucetOutcome =
  | "sent"
  | "rate_limited"
  | "challenge_required"
  | "refused"
  | "unreachable"
  | "unknown";

export type FaucetGateState =
  | "disabled"
  | "wallet_changed"
  | "no_pending_deploy"
  | "not_needed"
  | "waiting"
  | "requested";

export interface FaucetBudget {
  wallet: string;
  grantedAtMs: number;
  windowMs: number;
  maxPerWindow: number;
}

export interface FaucetLedgerEntry {
  atMs: number;
  wallet: string;
  origin: string;
  initcodeHash: string | null;
  needWei: string | null;
  balanceWei: string | null;
  outcome: FaucetOutcome;
  txHash: string | null;
  message: string;
  nextEligibleAtMs: number | null;
}

export interface FaucetHealth {
  reachable: boolean;
  ready: boolean | null;
  detail: string;
}

export interface FaucetStatus {
  enabled: boolean;
  budget: FaucetBudget | null;
  wallet: string | null;
  walletError: string | null;
  walletMatches: boolean;
  storeError: string | null;
  health: FaucetHealth;
  nextEligibleAtMs: number | null;
  faucetNextEligibleAtMs: number | null;
  faucetEligibilityKnown: boolean;
  ledger: FaucetLedgerEntry[];
  faucetUrl: string;
  faucetPage: string;
  deployGasLimit: number;
  dripWei: string;
  windowHours: number;
  maxPerWindow: number;
  pendingOwnerSignOff: string[];
  nowMs: number;
}

export interface FaucetResult {
  state: FaucetGateState;
  outcome: FaucetOutcome | null;
  txHash: string | null;
  nextEligibleAtMs: number | null;
  balanceWei: string | null;
  needWei: string | null;
  message: string;
  faucetPage: string;
}

/** SALT from a wei decimal string, to at most `digits` decimals (no float rounding). */
export function formatSalt(wei: string | null | undefined, digits = 6): string {
  if (!wei || !/^\d+$/.test(wei)) return "unknown";
  const padded = wei.padStart(19, "0");
  const whole = padded.slice(0, -18).replace(/^0+(?=\d)/, "");
  const frac = padded.slice(-18).slice(0, digits).replace(/0+$/, "");
  return frac ? `${whole}.${frac} SALT` : `${whole} SALT`;
}

export function outcomeLabel(o: FaucetOutcome): string {
  switch (o) {
    case "sent":
      return "sent";
    case "rate_limited":
      return "refused for now (limit)";
    case "challenge_required":
      return "CAPTCHA needed";
    case "refused":
      return "refused";
    case "unreachable":
      return "faucet unreachable";
    case "unknown":
      return "outcome unknown";
    default:
      return String(o);
  }
}

/** One plain line about the faucet service itself. */
export function healthLine(h: FaucetHealth): string {
  if (!h.reachable) return "Faucet unreachable. " + h.detail;
  if (h.ready === false) return h.detail;
  return h.detail;
}

/** The later of the app's own window and the faucet's answer, or null when the member may ask now. */
export function nextEligible(st: FaucetStatus): number | null {
  const times = [st.nextEligibleAtMs, st.faucetNextEligibleAtMs].filter(
    (t): t is number => typeof t === "number" && t > st.nowMs,
  );
  return times.length ? Math.max(...times) : null;
}

export function formatWhen(ms: number): string {
  try {
    return new Date(ms).toLocaleString();
  } catch {
    return String(ms);
  }
}

export function shortAddress(a: string | null): string {
  if (!a) return "no wallet";
  return a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a;
}

export interface FaucetApi {
  status(): Promise<FaucetStatus>;
  grant(): Promise<FaucetBudget>;
  revoke(): Promise<void>;
  request(initcodeHash: string): Promise<FaucetResult>;
  openChallenge(): Promise<string>;
}

export const tauriFaucet: FaucetApi = {
  status: () => invoke<FaucetStatus>("faucet_status"),
  grant: () => invoke<FaucetBudget>("faucet_grant"),
  revoke: () => invoke<void>("faucet_revoke"),
  request: (initcodeHash) =>
    invoke<FaucetResult>("faucet_request", { initcodeHash }),
  openChallenge: () => invoke<string>("faucet_open_challenge"),
};

/** The in-app faucet exists only in the desktop app (core makes the request). */
export const FAUCET_UNAVAILABLE_OUTSIDE_DESKTOP =
  "The in-app faucet is available in the Citrate Core desktop app.";

export function faucetApi(): FaucetApi | null {
  return BRIDGE_MODE === "tauri" ? tauriFaucet : null;
}
