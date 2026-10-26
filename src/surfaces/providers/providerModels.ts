// =====================================================================
// D-11 (RC52) — Settings › Models: the pure rules behind the provider cards and the spend view.
//
// Every value here is derived from what core reports through `bridge.providers` (Rust
// `providers_ipc.rs`): status (pinned vendor table, keychain presence, sealed guard file) and the
// usage summary (the signed usage log). Nothing is invented: an unknown amount stays unknown, a
// vendor with no adapter in this build says so, and a custom endpoint is Unverified everywhere.
// ADR-2026-10-26-byok-provider-governance P-4, P-7, P-9c, P-10.
// =====================================================================
import type { ProviderCaps, ProviderPeriodUsage, ProviderStatus, ProviderUsage } from "../../bridge/domains";
import { formatMicros, parseUsdToMicros } from "../../agent/escalation";

/** P-7c rotation reminder: a stored key older than this many days is flagged (pending owner sign-off). */
export const ROTATE_AFTER_DAYS = 90;
const DAY_MS = 86_400_000;

/** The one card state a provider is in, most urgent first. */
export type CardState =
  | "unavailable" // this build has no adapter for it
  | "unconfirmed" // a custom endpoint whose exact URL the member has not confirmed yet
  | "halted" // paused (by the member or automatically)
  | "capped" // a cap is reached: new calls are refused
  | "connected" // a key is stored
  | "disconnected"; // no key yet

export function capReached(u: ProviderUsage | undefined, caps: ProviderCaps): string | null {
  if (!u) return null;
  if (caps.dailyTokens !== null && u.today.tokens >= caps.dailyTokens) return "The daily token cap is reached. New calls are refused until 00:00 UTC.";
  if (caps.monthlyTokens !== null && u.month.tokens >= caps.monthlyTokens) return "The monthly token cap is reached. New calls are refused until the month ends (UTC).";
  if (caps.dailyMicros !== null && u.today.spendMicros >= caps.dailyMicros) return "The daily spending cap is reached. New calls are refused until 00:00 UTC.";
  if (caps.monthlyMicros !== null && u.month.spendMicros >= caps.monthlyMicros) return "The monthly spending cap is reached. New calls are refused until the month ends (UTC).";
  return null;
}

export function cardState(p: ProviderStatus, u: ProviderUsage | undefined): CardState {
  if (p.adapter === null) return "unavailable";
  if (p.kind === "custom" && !p.confirmed) return "unconfirmed";
  if (p.halt) return "halted";
  if (p.keyConfigured && capReached(u, p.caps)) return "capped";
  return p.keyConfigured ? "connected" : "disconnected";
}

/** Whole days since the key was stored, or null when core did not record when. */
export function keyAgeDays(keySetMs: number | null, nowMs: number): number | null {
  if (keySetMs === null || keySetMs <= 0 || nowMs < keySetMs) return null;
  return Math.floor((nowMs - keySetMs) / DAY_MS);
}

export function rotationDue(p: ProviderStatus, nowMs: number): number | null {
  if (!p.keyConfigured) return null;
  const age = keyAgeDays(p.keySetMs, nowMs);
  return age !== null && age >= ROTATE_AFTER_DAYS ? age : null;
}

/** What Resume does for this halt, said before the member presses it. */
export function resumeEffect(kind: string): { label: string; note: string } {
  switch (kind) {
    case "unknownModel":
      return { label: "Resume and approve this model", note: "Resuming approves the model named above for this provider, so it will not pause for it again." };
    case "guardUnreadable":
      return { label: "Resume and reset settings", note: "Resuming resets caps, prices and pauses for every provider to the defaults, and removes custom endpoints." };
    default:
      return { label: "Resume", note: "Resuming lets calls to this provider run again within your caps." };
  }
}

export function fmtTokens(n: number): string {
  return Math.max(0, Math.floor(n)).toLocaleString("en-US");
}

export function fmtWhen(ms: number): string {
  if (!ms) return "";
  return new Date(ms).toISOString().slice(0, 16).replace("T", " ") + " UTC";
}

export function fmtDay(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

/** "1,200 of 1,000,000 tokens" or "1,200 tokens (no cap)". */
export function tokenLine(used: number, cap: number | null): string {
  return cap === null ? `${fmtTokens(used)} tokens (no cap)` : `${fmtTokens(used)} of ${fmtTokens(cap)} tokens`;
}

/** Spend counts priced calls only; calls without a price have an unknown cost, never zero. */
export function spendLine(p: ProviderPeriodUsage, cap: number | null): string {
  const base = cap === null ? `${formatMicros(p.spendMicros)} spent (no cap)` : `${formatMicros(p.spendMicros)} of ${formatMicros(cap)} spent`;
  if (p.unpricedCalls === 0) return base;
  const n = p.unpricedCalls;
  return `${base}, plus ${n} call${n === 1 ? "" : "s"} with no price set (cost unknown)`;
}

// ---- caps editor ----

export interface CapsForm {
  dailyTokens: string;
  monthlyTokens: string;
  dailyUsd: string;
  monthlyUsd: string;
}

const microsToUsd = (m: number | null) => (m === null ? "" : formatMicros(m).slice(1));

export function capsToForm(c: ProviderCaps): CapsForm {
  return {
    dailyTokens: c.dailyTokens === null ? "" : String(c.dailyTokens),
    monthlyTokens: c.monthlyTokens === null ? "" : String(c.monthlyTokens),
    dailyUsd: microsToUsd(c.dailyMicros),
    monthlyUsd: microsToUsd(c.monthlyMicros),
  };
}

function tokensField(v: string): number | null | "bad" {
  const t = v.trim().replace(/[,_\s]/g, "");
  if (t === "") return null;
  if (!/^\d{1,13}$/.test(t)) return "bad";
  return Number(t);
}

function usdField(v: string): number | null | "bad" {
  if (v.trim() === "") return null;
  const m = parseUsdToMicros(v);
  return m === null ? "bad" : m;
}

/** Blank means no cap of that kind. Core validates again (and holds the upper limits). */
export function formToCaps(f: CapsForm): { caps: ProviderCaps } | { error: string } {
  const dailyTokens = tokensField(f.dailyTokens);
  const monthlyTokens = tokensField(f.monthlyTokens);
  const dailyMicros = usdField(f.dailyUsd);
  const monthlyMicros = usdField(f.monthlyUsd);
  if (dailyTokens === "bad" || monthlyTokens === "bad") return { error: "Token caps are whole numbers, for example 1000000. Leave one blank for no cap." };
  if (dailyMicros === "bad" || monthlyMicros === "bad") return { error: "Spending caps are dollar amounts, for example 5 or 0.50. Leave one blank for no cap." };
  if (dailyTokens !== null && monthlyTokens !== null && dailyTokens > monthlyTokens) return { error: "The daily token cap is above the monthly one." };
  if (dailyMicros !== null && monthlyMicros !== null && dailyMicros > monthlyMicros) return { error: "The daily spending cap is above the monthly one." };
  return { caps: { dailyTokens, monthlyTokens, dailyMicros, monthlyMicros } };
}

// ---- custom endpoint ----

export interface CustomForm {
  label: string;
  baseUrl: string;
  models: string;
}

export const EMPTY_CUSTOM: CustomForm = { label: "", baseUrl: "", models: "" };

export function customToInput(f: CustomForm): { label: string; baseUrl: string; models: string[] } | { error: string } {
  const label = f.label.trim();
  const baseUrl = f.baseUrl.trim();
  const models = f.models
    .split(/[,\s]+/)
    .map((m) => m.trim())
    .filter(Boolean);
  if (!label) return { error: "Give the endpoint a name." };
  if (!/^https:\/\/[^\s/]+/.test(baseUrl)) return { error: "The endpoint must start with https://." };
  if (models.length === 0) return { error: "List at least one model the endpoint serves." };
  return { label, baseUrl, models };
}

/** The member must type the exact URL; nothing is trimmed or normalised for them. */
export function confirmMatches(typed: string, baseUrl: string): boolean {
  return typed === baseUrl;
}

// ---- key hygiene (P-7c) ----

/** The steps every vendor shares. `host` is the pinned host core reports. */
export function hygieneSteps(name: string, host: string): string[] {
  return [
    `Create a new ${name} key used only by Citrate Core, so you can revoke it without breaking anything else.`,
    "Limit it at the vendor where you can: its own project or workspace, model access only, and a monthly spending limit.",
    `Paste it here once. Core tests it with ${host}, then keeps it in this computer's keychain. It is never shown again and never sent to Citrate.`,
  ];
}

/** Vendor-specific advice, worded as what to look for (the vendor's console is the authority). */
export const VENDOR_HINT: Record<string, string> = {
  openai: "OpenAI: create the key inside a project made for Citrate Core, and set that project's budget and model permissions.",
  anthropic: "Anthropic: create the key in a workspace made for Citrate Core, and set the workspace spend limit.",
  gemini: "Google: restrict the key to the Generative Language API. Google Cloud budgets send alerts but do not stop calls, so keep the caps below.",
  xai: "xAI: create the key in the xAI console and limit the models and endpoints it may use.",
  deepseek: "DeepSeek charges a prepaid balance, so top up only what you are willing to spend.",
  meta: "Meta: create the key in the Llama developer console, in a project used only by Citrate Core.",
};

/** Optional subtitle for a vendor's card. */
export const VENDOR_SUBTITLE: Record<string, string> = {
  meta: "Muse models on the Meta Model API",
};
