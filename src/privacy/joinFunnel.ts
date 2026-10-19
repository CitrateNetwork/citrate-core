// =====================================================================
// 0.5.2 Join — anonymous step-reached funnel events, through the EXISTING opt-in telemetry.
//
// No new endpoint and no new consent surface. The existing telemetry is off by default and sends
// nothing in the background: a member prepares a diagnostic report, reviews the exact JSON and
// presses Send (DiagnosticReport, the ConsentGate). Join steps ride inside that same reviewed
// report as one more disclosed field (`joinSteps`, listed in telemetry-fields.json).
//
// What a step record is: the bare step id. Nothing else. No invite code, group id, cluster name,
// wallet address, email, per-install id or time. The pipeline already carries the app version
// and OS family; its only id is the random per-report `reportId`, so no identifier is added here.
//
// Rules this module enforces (joinFunnel.test.ts):
//   - telemetry off: `reachedStep` records nothing and `pendingJoinSteps` returns nothing;
//   - only allowlisted step ids are kept (the Rust send path re-checks the same list);
//   - one record per install per step: a step already pending, or already sent in a report, is
//     never queued again.
// Data source (Rule 7): the telemetry toggle in app state (Settings › Telemetry), and this
// install's own local storage for the pending / sent step sets.
// =====================================================================
import { STORAGE_KEY } from "../shell/state";
import fields from "./telemetry-fields.json";

/** The disclosure version that added `joinSteps` (telemetry-fields.json `version`). A consent
 *  recorded at this version or later saw the field on the consent screen. */
export const JOIN_STEPS_CONSENT_VERSION: number = fields.version;
/** The marker written for a consent given before 0.5.2 (or one with no marker at all). */
export const PRE_052_CONSENT_VERSION = 1;

/** Whether join steps may be recorded: telemetry is on AND the member consented to (or
 *  acknowledged the note about) the disclosure that lists `joinSteps`. A missing marker counts
 *  as pre-0.5.2. */
export function funnelConsented(telemetry: boolean, consentVersion: number | null | undefined): boolean {
  return telemetry === true && (consentVersion ?? PRE_052_CONSENT_VERSION) >= JOIN_STEPS_CONSENT_VERSION;
}

/** Whether the one-time note is due: telemetry was turned on under an older disclosure. */
export function needsJoinStepsNote(telemetry: boolean, consentVersion: number | null | undefined): boolean {
  return telemetry === true && (consentVersion ?? PRE_052_CONSENT_VERSION) < JOIN_STEPS_CONSENT_VERSION;
}

/** The state patch for turning telemetry on or off through the consent screen (or the note). */
export function telemetryConsentPatch(on: boolean): { telemetry: boolean; telemetryConsentVersion: number | null } {
  return { telemetry: on, telemetryConsentVersion: on ? JOIN_STEPS_CONSENT_VERSION : null };
}

/** The join path, in order. Mirrors `JOIN_STEPS` in src-tauri/src/telemetry.rs. */
export const JOIN_STEPS = [
  "arrive",
  "download",
  "open_app",
  "sign_in",
  "kyc",
  "membership",
  "wallet",
  "bond",
  "node_synced",
  "activate_validator",
  "join_group",
] as const;

export type JoinStep = (typeof JOIN_STEPS)[number];

/** Local storage key for this install's step sets. */
export const JOIN_FUNNEL_STORAGE_KEY = "citrate-core-join-funnel-v1";

const ORDER = new Map<string, number>(JOIN_STEPS.map((s, i) => [s, i]));
const isStep = (s: unknown): s is JoinStep => typeof s === "string" && ORDER.has(s);
const inPathOrder = (xs: Iterable<JoinStep>): JoinStep[] =>
  [...new Set(xs)].sort((a, b) => (ORDER.get(a) ?? 0) - (ORDER.get(b) ?? 0));

/** The persisted telemetry toggle, read without the store (default off). */
function persistedConsent(): boolean {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const r = raw ? JSON.parse(raw) : null;
    return r ? funnelConsented(Boolean(r.telemetry), r.telemetryConsentVersion) : false;
  } catch {
    return false;
  }
}

let consentSource: (() => boolean) | null = null;
// The last consent change made this session (Settings or the note). It wins over the persisted
// app state, which can be stale when that state's own write failed.
let rememberedConsent: { telemetry: boolean; telemetryConsentVersion: number | null } | null = null;

/** Record a consent change made this session (pass `telemetryConsentPatch(on)`). */
export function rememberTelemetryConsent(c: { telemetry: boolean; telemetryConsentVersion: number | null }): void {
  rememberedConsent = { telemetry: c.telemetry, telemetryConsentVersion: c.telemetryConsentVersion };
}

/** Bind the live consent (the app binds `funnelConsented(telemetry, telemetryConsentVersion)`).
 *  `null` falls back to the persisted toggle. */
export function setJoinFunnelConsent(source: (() => boolean) | null): void {
  consentSource = source;
}

function consented(): boolean {
  try {
    if (consentSource) return consentSource() === true;
    if (rememberedConsent) return funnelConsented(rememberedConsent.telemetry, rememberedConsent.telemetryConsentVersion);
    return persistedConsent();
  } catch {
    return false;
  }
}

interface Record_ {
  v: 1;
  pending: JoinStep[];
  sent: JoinStep[];
}

// In-memory copy of the step sets. It is the source while storage is blocked, and — once any
// write has failed (a full quota, say) — for the rest of the session (`writeFailed` is sticky),
// so a stale persisted record can never undo dedupe after a write was lost.
let memory: Record_ | null = null;
let writeFailed = false;

const EMPTY = (): Record_ => ({ v: 1, pending: [], sent: [] });

function readPersisted(): Record_ | "blocked" {
  let raw: string | null;
  try {
    raw = localStorage.getItem(JOIN_FUNNEL_STORAGE_KEY);
  } catch {
    return "blocked";
  }
  if (!raw) return EMPTY();
  try {
    const r = JSON.parse(raw) as Partial<Record_>;
    return {
      v: 1,
      pending: inPathOrder((Array.isArray(r.pending) ? r.pending : []).filter(isStep)),
      sent: inPathOrder((Array.isArray(r.sent) ? r.sent : []).filter(isStep)),
    };
  } catch {
    return EMPTY(); // unreadable: start over rather than guess
  }
}

function load(): Record_ {
  // In-memory over persisted: after a failed write (or with storage blocked) memory is the truth.
  if (writeFailed && memory) return memory;
  const persisted = readPersisted();
  if (persisted === "blocked") return memory ?? EMPTY();
  return persisted;
}

function store(r: Record_): void {
  memory = r;
  try {
    localStorage.setItem(JOIN_FUNNEL_STORAGE_KEY, JSON.stringify({ v: 1, pending: r.pending, sent: r.sent }));
  } catch {
    writeFailed = true; // sticky: memory wins from now on this session
  }
}

/**
 * Note that this install reached a join step. A no-op unless telemetry is on, and a no-op for a
 * step already pending or already sent. Never sends anything: the step waits for the member's
 * next reviewed diagnostic report. Safe to call from anywhere (it never throws).
 */
export function reachedStep(step: JoinStep): void {
  if (!isStep(step) || !consented()) return;
  const r = load();
  if (r.pending.includes(step) || r.sent.includes(step)) return;
  store({ v: 1, pending: inPathOrder([...r.pending, step]), sent: r.sent });
}

/** The steps waiting to ride in the next reviewed report, in path order. Empty when telemetry is off. */
export function pendingJoinSteps(): JoinStep[] {
  if (!consented()) return [];
  return [...load().pending];
}

/** After a report was sent: move its steps from pending to sent so they are never sent twice. */
export function markJoinStepsSent(steps: readonly string[]): void {
  const done = steps.filter(isStep);
  if (!done.length) return;
  const r = load();
  store({
    v: 1,
    pending: r.pending.filter((s) => !done.includes(s)),
    sent: inPathOrder([...r.sent, ...done]),
  });
}

/** Forget every recorded step (called when the member turns telemetry off). */
export function clearJoinSteps(): void {
  memory = EMPTY();
  try {
    localStorage.removeItem(JOIN_FUNNEL_STORAGE_KEY);
  } catch {
    writeFailed = true; // the stale record stays on disk; the empty memory copy wins
  }
}

/** The join step an onboarding stage reaches, or null (S0 is `open_app`, recorded at launch;
 *  S6 is node ignition, and the step that counts there is `node_synced`). */
export function stepForStage(stage: string): JoinStep | null {
  switch (stage) {
    case "s1":
      return "sign_in";
    case "s2":
      return "kyc";
    case "s3":
      return "membership";
    case "s4":
      return "wallet";
    case "s5":
      return "bond";
    default:
      return null;
  }
}
