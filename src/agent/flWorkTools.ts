// =====================================================================
// 0.5.4 Hermes × FL, WP-H3: worker terms, consent and eligibility tools (gap analysis §5.2).
//
//   fl_terms_read       the member's sealed worker terms, or "not set"             (read)
//   fl_consent_list     per-round consents and the conversation-training switch    (read)
//   fl_consent_withdraw withdraw one round's consent, or turn conversation training off (no card: lowers authority)
//   fl_eligibility      this machine against a requirements object, or an offer   (read)
//
// Data sources (Rule 7):
//  - terms: core's fl_work.rs (lane 1, FL design board 11), `<app data>/fl-work/terms.sealed`
//    under the work key in the OS keyring, through src/fl/workerTerms.ts. Read only here: the one
//    terms write path is lane 3's worker_terms_propose and its save-receipt card (#693).
//  - consent: core's fl_rounds.rs (fl_overview: the device worker's `fl_consent.json` and the start
//    records; fl_round_consent_revoke) and fl_trajectories.rs (trajectories_settings_get, and the
//    turn-off-only trajectories_consent_revoke, which takes no switch argument). The
//    conversation switch is a setting Hermes reads at start, so it is written under the start lock.
//  - eligibility: core's tier.rs (tier_recommend, a local probe: RAM, unified memory, NVIDIA VRAM,
//    free disk; no network). An offer's requirements come from WorkMarket (`requirementsHash`, the
//    file pinned by `requirementsCid`), which this network's address book does not pin, so an
//    offer id answers "Not available yet on this network" from the book, never a guess.
//
// Unknown is not zero: a fact the probe could not read is "not known" with its reason, and a
// requirement that depends on it is neither met nor failed.
// =====================================================================
import type { ToolAnnotation } from "./toolAnnotations";
import type { FlOverview, FlTrajectoryStatus, TierReport } from "../bridge/domains";
import type { WorkerTerms } from "../fl/workerTerms";
import { NOT_AVAILABLE, pinOf, type FlBook } from "../fl/networkState";
import { writeStartSettingUnlessStarting } from "./startSettingsWrite";
import { HELD_DURING_START, noteRunning } from "../shell/slices/hermesRunning";

export const FL_WORK_TOOL_NAMES = ["fl_terms_read", "fl_consent_list", "fl_consent_withdraw", "fl_eligibility"] as const;
export type FlWorkToolName = (typeof FL_WORK_TOOL_NAMES)[number];

export function isFlWorkTool(name: string): name is FlWorkToolName {
  return (FL_WORK_TOOL_NAMES as readonly string[]).includes(name);
}

export interface FlWorkDeps {
  terms: { load(): Promise<WorkerTerms | null> };
  flRounds: {
    overview(): Promise<FlOverview>;
    revokeConsent(roundId: string): Promise<string[]>;
    trajectoryStatus(): Promise<FlTrajectoryStatus>;
    /** Core's turn-off-only command (trajectories_consent_revoke); there is no way to turn it on here. */
    revokeTrajectoryConsent(): Promise<FlTrajectoryStatus>;
  };
  tier: { recommend(): Promise<TierReport | null> };
  /** The shipped address book (src-tauri/addresses/40204.json). */
  book: FlBook;
  toast: (text: string) => void;
}

export interface FlWorkOutcome {
  status: string;
  result: string;
}

const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));
const str = (v: unknown): string => (typeof v === "string" ? v.trim() : "");
const ROUND_ID = /^0x[0-9a-fA-F]{64}$/;
const GB = 1024 ** 3;

function bool(v: unknown): boolean | null {
  if (v === true || v === "true") return true;
  if (v === false || v === "false") return false;
  return null;
}

function hoursWords(h: WorkerTerms["hours"]): string {
  return h === "any" ? "any time" : `${h.start} to ${h.end}`;
}

/** The terms in the member's words, one row each. */
export function termsRows(t: WorkerTerms): { k: string; v: string }[] {
  return [
    { k: "Floor", v: `${t.floorSaltPerUnit} SALT per unit` },
    { k: "Hours", v: hoursWords(t.hours) },
    { k: "Weekdays only", v: t.weekdaysOnly ? "yes" : "no" },
    { k: "Only on mains power", v: t.mainsOnly ? "yes" : "no" },
    { k: "Data classes rounds may read", v: t.dataClasses.length ? t.dataClasses.join(", ") : "none" },
  ];
}

export async function runFlWorkTool(name: FlWorkToolName, args: Record<string, unknown>, _ann: ToolAnnotation | null, d: FlWorkDeps): Promise<FlWorkOutcome> {
  switch (name) {
    case "fl_terms_read":
      return termsRead(d);
    case "fl_consent_list":
      return consentList(d);
    case "fl_consent_withdraw":
      return consentWithdraw(args, d);
    case "fl_eligibility":
      return eligibility(args, d);
  }
}

async function termsRead(d: FlWorkDeps): Promise<FlWorkOutcome> {
  try {
    const t = await d.terms.load();
    if (!t) return { status: "ok", result: "No worker terms are set. They are sealed on this device once the member approves them on a terms card or in Settings › Work." };
    return {
      status: "ok",
      result:
        "The member's worker terms (sealed on this device, never published; only the floor is enforced on chain, by WorkMarket join): " +
        termsRows(t).map((r) => `${r.k}: ${r.v}`).join("; ") +
        ".",
    };
  } catch (e) {
    return { status: "ok", result: "The worker terms could not be read: " + errText(e) };
  }
}

async function consentList(d: FlWorkDeps): Promise<FlWorkOutcome> {
  const parts: string[] = [];
  try {
    const o = await d.flRounds.overview();
    if (o.consentError) parts.push("Round consents: not known (" + o.consentError + ").");
    else if (o.consentedRounds.length === 0) parts.push("Round consents: none.");
    else parts.push("Round consents (the device worker reads these): " + o.consentedRounds.join(", ") + ".");
    const withdrawn = o.starts.filter((s) => s.roundId && s.revokedAtMs);
    if (withdrawn.length) parts.push("Withdrawn: " + withdrawn.map((s) => `${s.roundId} on ${new Date(s.revokedAtMs!).toISOString()}`).join(", ") + ".");
  } catch (e) {
    parts.push("Round consents: not known (" + errText(e) + ").");
  }
  try {
    const t = await d.flRounds.trajectoryStatus();
    if (t.loadError) parts.push("Training on verified conversations: not known (" + t.loadError + ").");
    else parts.push(`Training on verified conversations: ${t.settings.enabled ? "on" : "off"} (${t.recordedFiles} recorded, ${t.datasets} assembled training sets).`);
  } catch (e) {
    parts.push("Training on verified conversations: not known (" + errText(e) + ").");
  }
  parts.push("Dataset consents: this build has no dataset store, so there are none.");
  return { status: "ok", result: parts.join(" ") };
}

async function consentWithdraw(args: Record<string, unknown>, d: FlWorkDeps): Promise<FlWorkOutcome> {
  const scope = str(args.scope);
  if (scope === "round") {
    const roundId = str(args.roundId);
    if (!ROUND_ID.test(roundId)) return { status: "ok", result: "Name the round (0x and 64 hex digits, from fl_consent_list); nothing was withdrawn." };
    try {
      const left = await d.flRounds.revokeConsent(roundId);
      d.toast("Consent withdrawn for that round. The device worker stops taking its jobs.");
      return { status: "ok", result: `Withdrew this device's consent for round ${roundId}. Rounds still consented: ${left.length ? left.join(", ") : "none"}.` };
    } catch (e) {
      return { status: "ok", result: "The consent was not withdrawn: " + errText(e) };
    }
  }
  if (scope === "conversations") {
    try {
      // Hermes reads this switch at start: the write goes under the start-time settings lock, as
      // Settings' own toggle does (src/fl/TrajectoryConsent.tsx); during a start it is held.
      const s = await writeStartSettingUnlessStarting(async () => {
        const fresh = await d.flRounds.trajectoryStatus();
        noteRunning("learn", fresh.settings.enabled);
        return d.flRounds.revokeTrajectoryConsent();
      });
      if (!s) return { status: "ok", result: HELD_DURING_START + " Conversation training was not changed; try again once Hermes is up." };
      d.toast("Training on your conversations is off.");
      return {
        status: "ok",
        result: `Turned off training on verified conversations; assembled training sets were deleted and ${s.recordedFiles} recorded conversations were kept (the member can delete them in Settings).`,
      };
    } catch (e) {
      return { status: "ok", result: "Conversation training was not turned off: " + errText(e) };
    }
  }
  return { status: "ok", result: 'Say scope "round" (with roundId) or "conversations"; nothing was withdrawn.' };
}

type Verdict = { need: string; have: string; ok: boolean | null };

function gb(bytes: number | null): string {
  return bytes == null ? "not known" : `${Math.floor((bytes / GB) * 10) / 10} GB`;
}

function num(v: unknown): number | null {
  const n = typeof v === "number" ? v : typeof v === "string" && v.trim() !== "" ? Number(v) : NaN;
  return Number.isFinite(n) && n >= 0 ? n : null;
}

/** This machine against a requirements object. A fact the probe could not read decides nothing. */
export function checkRequirements(req: Record<string, unknown>, r: TierReport): Verdict[] {
  const f = r.facts;
  const out: Verdict[] = [];
  const atLeast = (need: number | null, have: number | null, label: string) => {
    if (need == null) return;
    out.push({ need: `${label} at least ${need} GB`, have: `${label} ${gb(have)}`, ok: have == null ? null : have >= need * GB });
  };
  atLeast(num(req.minRamGb), f.totalRamBytes, "memory");
  // Apple Silicon's GPU uses system memory; elsewhere only a probed NVIDIA card counts.
  const vram = f.unifiedMemory === true ? f.totalRamBytes : f.gpuVramBytes;
  atLeast(num(req.minVramGb), vram, f.unifiedMemory === true ? "GPU memory (shared)" : "GPU memory");
  atLeast(num(req.minDiskGb), f.diskFreeBytes, "free disk");
  if (bool(req.accelerator) === true) {
    const acc = f.unifiedMemory === true || (f.gpuVramBytes ?? 0) > 0 ? true : f.unifiedMemory === false && f.gpuVramBytes == null ? false : null;
    out.push({ need: "a GPU or Apple Silicon", have: acc === true ? "yes" : acc === false ? "no GPU found" : "not known", ok: acc });
  }
  if (bool(req.mainsOnly) === true) out.push({ need: "on mains power", have: "not known: this build does not read the battery", ok: null });
  return out;
}

async function eligibility(args: Record<string, unknown>, d: FlWorkDeps): Promise<FlWorkOutcome> {
  if (str(args.offerId)) {
    if (!pinOf(d.book, "WorkMarket")) {
      return { status: "ok", result: `${NOT_AVAILABLE}: WorkMarket is not deployed here, so there is no offer to check. Pass a requirements object to check this machine against it.` };
    }
    return { status: "ok", result: "Per-offer eligibility reads the offer's requirements file from WorkMarket; this build checks a requirements object only. Pass one." };
  }
  const req = args.requirements && typeof args.requirements === "object" && !Array.isArray(args.requirements) ? (args.requirements as Record<string, unknown>) : null;
  if (!req) return { status: "ok", result: "Pass requirements (minRamGb, minVramGb, minDiskGb, accelerator, mainsOnly) or an offerId; nothing was checked." };
  let report: TierReport | null;
  try {
    report = await d.tier.recommend();
  } catch (e) {
    return { status: "ok", result: "This machine could not be probed: " + errText(e) };
  }
  if (!report) return { status: "ok", result: "This machine can be probed only in the desktop app; nothing was checked." };
  const v = checkRequirements(req, report);
  if (v.length === 0) return { status: "ok", result: "Those requirements name nothing this build can check (minRamGb, minVramGb, minDiskGb, accelerator, mainsOnly)." };
  const failed = v.filter((x) => x.ok === false);
  const unknown = v.filter((x) => x.ok === null);
  const head = failed.length ? "Not eligible." : unknown.length ? "Not known: some facts could not be read." : "Eligible.";
  const lines = v.map((x) => `${x.ok === true ? "met" : x.ok === false ? "why not" : "not known"}: needs ${x.need}; this machine has ${x.have}`);
  return { status: "ok", result: `${head} From this machine's local probe (${report.facts.os} ${report.facts.arch}, tier ${report.effective}): ${lines.join("; ")}.` };
}
