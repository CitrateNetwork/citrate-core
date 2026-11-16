// =====================================================================
// citrate-core: a round draft (FL design board 07, Open a round)
//
// A draft is the four steps' answers, kept on this device until it is signed or discarded. It is
// not a round: nothing is escrowed or published by saving it. Hermes (in a group chat) may hand a
// draft in through `setRoundDraft(draft, "hermes")`; the member then checks every field and signs
// it themselves (HIC-1). Stored in this webview's localStorage; a failed read or write is said,
// never hidden.
// =====================================================================

export type RoundTemplate = "tune" | "eval" | "data";
export type WhoMayJoin = "members" | "group" | "anyone";

export interface RoundDraft {
  groupId: string | null;
  template: RoundTemplate;
  dataClass: string;
  /** Decimal SALT. */
  budgetSalt: string;
  /** Decimal SALT per unit. */
  rateSalt: string;
  maxWorkers: number;
  /** YYYY-MM-DD. */
  deadline: string;
  who: WhoMayJoin;
  /** The coordinator URL, or null for none chosen. */
  coordinator: string | null;
  source: "member" | "hermes";
  /** Who Hermes drafted it for and when, for the banner. */
  draftedFor?: string;
  savedAt?: number;
}

export const BLANK_DRAFT: RoundDraft = {
  groupId: null,
  template: "tune",
  dataClass: "",
  budgetSalt: "600",
  rateSalt: "1.50",
  maxWorkers: 8,
  deadline: "",
  who: "members",
  coordinator: null,
  source: "member",
};

const KEY = "citrate.fl.roundDraft.v1";

export function loadRoundDraft(): RoundDraft | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const d = JSON.parse(raw) as Partial<RoundDraft>;
    return { ...BLANK_DRAFT, ...d };
  } catch {
    return null;
  }
}

/** Save a draft on this device. Returns the saved time. Throws when storage refuses. */
export function saveRoundDraft(d: RoundDraft): number {
  const at = Date.now();
  localStorage.setItem(KEY, JSON.stringify({ ...d, savedAt: at }));
  return at;
}

/** Hand a draft in (Hermes or a group card). The member still checks and signs every field. */
export function setRoundDraft(d: Partial<RoundDraft>, source: RoundDraft["source"] = "member"): void {
  saveRoundDraft({ ...BLANK_DRAFT, ...d, source });
}

export function clearRoundDraft(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* nothing stored */
  }
}

/** How many leases the budget covers at the rate: floor(budget / rate), exact in wei. */
export function leasesCovered(budgetSalt: string, rateSalt: string): number | null {
  const w = (s: string) => {
    const m = /^(\d{1,12})(?:\.(\d{1,18}))?$/.exec(s.trim());
    return m ? BigInt(m[1]) * 10n ** 18n + BigInt((m[2] ?? "").padEnd(18, "0") || "0") : null;
  };
  const b = w(budgetSalt);
  const r = w(rateSalt);
  if (b === null || r === null || r === 0n) return null;
  return Number(b / r);
}

/** Why a draft cannot be opened as it stands, or null. */
export function draftProblems(d: RoundDraft): string[] {
  const out: string[] = [];
  if (!d.groupId) out.push("Choose a group");
  if (!d.dataClass.trim()) out.push("Name the data class members train on");
  if (leasesCovered(d.budgetSalt, d.rateSalt) === null) out.push("Budget and rate must be SALT amounts like 600 and 1.50");
  else if ((leasesCovered(d.budgetSalt, d.rateSalt) ?? 0) < 1) out.push("The budget must cover at least one lease");
  if (!(d.maxWorkers >= 3)) out.push("At least three workers (the round ledger's floor)");
  if (!/^\d{4}-\d{2}-\d{2}$/.test(d.deadline)) out.push("Set a deadline");
  return out;
}
