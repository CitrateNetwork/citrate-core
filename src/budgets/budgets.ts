// HUP-S2.3 — Settings → Budgets: types, pure helpers and the bridge to core.
//
// ADR-2026-09-30 (Rule-3 budgetable signatures, accepted). A web-signing budget lets Hermes sign
// in with Ethereum (EIP-4361) on ONE https site the member chose, a bounded number of times,
// until it expires or is revoked. Core owns the store, the checks and the signer; this file only
// reads and changes budgets through core's commands. The agent has no route to these commands.
import { BRIDGE_MODE } from "../bridge/mode";
import { invoke } from "../bridge/tauri/invoke";

export type BudgetStatus =
  "active" | "revoked" | "expired" | "used_up" | "wallet_changed" | string;

export interface BudgetView {
  id: number;
  origin: string;
  principal: string;
  chainId: number;
  maxCount: number;
  usedCount: number;
  remaining: number;
  grantedAtMs: number;
  expiresAtMs: number;
  revokedAtMs: number | null;
  status: BudgetStatus;
}

export type RecordKind =
  | "budget_granted"
  | "budget_revoked"
  | "all_budgets_revoked"
  | "store_reset"
  | "auto_sign";
export type RecordStatus =
  "reserved" | "signed" | "not_signed" | "outcome_unknown" | "final";

/** A hash-chained decision record (snake_case keys come straight from core). */
export interface DecisionRecord {
  recordId: number;
  kind: RecordKind;
  budgetId: number | null;
  principal: string;
  origin: string;
  payloadDigest: string | null;
  statement: string | null;
  nonce: string | null;
  requestId: string | null;
  signerAddress: string | null;
  atMs: number;
  remainingAfter: number | null;
  note: string | null;
  prevHash: string;
  hash: string;
  status: RecordStatus;
}

export interface WebBudgetStatus {
  snapshot: {
    health: { state: "ok" } | { state: "failed"; reason: string };
    budgets: BudgetView[];
    records: DecisionRecord[];
    headHash: string;
    recordCount: number;
  };
  attestation: { available: boolean; reason: string };
  defaults: { maxCount: number; ttlDays: number; pendingOwnerSignoff: boolean };
  ceilings: { maxCount: number; ttlDays: number };
  rate: { minGapSeconds: number; windowMax: number; windowHours: number };
  walletAddress: string | null;
  nowMs: number;
}

/** Core serializes the record with snake_case field names; map them for the UI. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function normalizeRecord(r: any): DecisionRecord {
  return {
    recordId: r.recordId ?? r.record_id,
    kind: r.kind,
    budgetId: r.budgetId ?? r.budget_id ?? null,
    principal: r.principal,
    origin: r.origin,
    payloadDigest: r.payloadDigest ?? r.payload_digest ?? null,
    statement: r.statement ?? null,
    nonce: r.nonce ?? null,
    requestId: r.requestId ?? r.request_id ?? null,
    signerAddress: r.signerAddress ?? r.signer_address ?? null,
    atMs: r.atMs ?? r.at_ms,
    remainingAfter: r.remainingAfter ?? r.remaining_after ?? null,
    note: r.note ?? null,
    prevHash: r.prevHash ?? r.prev_hash,
    hash: r.hash,
    status: r.status,
  };
}

export function normalizeStatus(s: WebBudgetStatus): WebBudgetStatus {
  return {
    ...s,
    snapshot: {
      ...s.snapshot,
      records: (s.snapshot.records ?? []).map(normalizeRecord),
    },
  };
}

/** "2d 3h", "4h 12m", "3m 05s", "59s", or "expired". */
export function formatCountdown(msLeft: number): string {
  if (!(msLeft > 0)) return "expired";
  const s = Math.floor(msLeft / 1000);
  const d = Math.floor(s / 86_400);
  const h = Math.floor((s % 86_400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${String(sec).padStart(2, "0")}s`;
  return `${sec}s`;
}

export function statusLabel(s: BudgetStatus): string {
  switch (s) {
    case "active":
      return "Active";
    case "revoked":
      return "Revoked";
    case "expired":
      return "Expired";
    case "used_up":
      return "Used up";
    case "wallet_changed":
      return "Different wallet";
    default:
      return "Unknown";
  }
}

export function recordLabel(k: RecordKind): string {
  switch (k) {
    case "budget_granted":
      return "Budget granted";
    case "budget_revoked":
      return "Budget revoked";
    case "all_budgets_revoked":
      return "All budgets revoked";
    case "store_reset":
      return "Budgets reset";
    case "auto_sign":
      return "Signed for you";
    default:
      return "Decision";
  }
}

export function recordStatusLabel(s: RecordStatus): string {
  switch (s) {
    case "signed":
      return "signed";
    case "not_signed":
      return "not signed";
    case "reserved":
      return "in progress";
    case "outcome_unknown":
      return "outcome unknown (the app stopped mid-signature)";
    default:
      return "";
  }
}

/** Client-side mirror of core's checks, for an early, readable error. Core re-checks everything. */
export function validateGrantForm(
  origin: string,
  maxCount: number,
  ttlDays: number,
  ceilings: { maxCount: number; ttlDays: number },
): string | null {
  let u: URL;
  try {
    u = new URL(origin.trim());
  } catch {
    return "Enter a valid site address, for example https://app.example.org";
  }
  if (u.protocol !== "https:")
    return "Only https sites can have a sign-in budget";
  if (u.username || u.password)
    return "Enter the site origin only, without a user name";
  if ((u.pathname && u.pathname !== "/") || u.search || u.hash)
    return "Enter the site origin only, without a path";
  const host = u.hostname.toLowerCase();
  if (/^\d+\.\d+\.\d+\.\d+$/.test(host) || host.startsWith("["))
    return "IP addresses cannot have a sign-in budget";
  if (host === "localhost" || host.endsWith(".localhost"))
    return "Local addresses cannot have a sign-in budget";
  if (!host.includes(".")) return "The host must be a full domain name";
  if (
    !Number.isInteger(maxCount) ||
    maxCount < 1 ||
    maxCount > ceilings.maxCount
  ) {
    return `A budget allows 1 to ${ceilings.maxCount} sign-ins`;
  }
  if (!Number.isInteger(ttlDays) || ttlDays < 1 || ttlDays > ceilings.ttlDays) {
    return `A budget lasts 1 to ${ceilings.ttlDays} days`;
  }
  return null;
}

export interface BudgetsApi {
  status(): Promise<WebBudgetStatus>;
  grant(origin: string, maxCount: number, ttlDays: number): Promise<BudgetView>;
  revoke(id: number): Promise<void>;
  revokeAll(): Promise<number>;
  reset(): Promise<void>;
}

export const tauriBudgets: BudgetsApi = {
  status: () => invoke<WebBudgetStatus>("web_budget_status"),
  grant: (origin, maxCount, ttlDays) =>
    invoke<BudgetView>("web_budget_grant", { origin, maxCount, ttlDays }),
  revoke: (id) => invoke<void>("web_budget_revoke", { id }),
  revokeAll: () => invoke<number>("web_budget_revoke_all"),
  reset: () => invoke<void>("web_budget_reset"),
};

/** Budgets exist only in the desktop app (the store and the signer live in core). */
export const BUDGETS_UNAVAILABLE_OUTSIDE_DESKTOP =
  "Budgets are managed in the Citrate Core desktop app.";

export function budgetsApi(): BudgetsApi | null {
  return BRIDGE_MODE === "tauri" ? tauriBudgets : null;
}
