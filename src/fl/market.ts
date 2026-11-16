// =====================================================================
// citrate-core: WorkMarket offers for Rounds › Matches (FL design board 05, 0.6.0 states)
//
// Data source (Rule 7): core's `fl_market_offers` (FLM-CORE-1, src-tauri fl_market.rs, PR #625):
// WorkMarket OfferOpened/OfferClosed logs, getOffer(id), the requirements file verified against its
// hash, and the group-admin check against the live roster. The shapes below mirror its serde
// output. That command is not in the 0.5.4 build and WorkMarket is not on r1007, so the app only
// asks for offers once the network state shows WorkMarket deployed; a failed ask is shown as "—"
// with its reason.
//
// Matching is local and plain: the member's terms (workerTerms.ts) and this machine's memory
// (tier.rs hardware facts) against each offer's verified requirements. Each match says why, and
// each offer that cannot be joined says why not. No projection, no ranking by amount.
// =====================================================================
import { BRIDGE_MODE } from "../bridge/mode";
import { invoke } from "../bridge/tauri/invoke";
import type { HardwareFacts } from "../bridge/domains";
import type { WorkerTerms } from "./workerTerms";
import { saltFromWei } from "./format";

export type OfferState = "open" | "running" | "payoutPosted" | "disputed" | "closed" | "expired";

export interface OpenedEvent {
  id: number;
  kind: "flRound" | "task";
  poster: string;
  groupHash: string;
  /** Wei per unit, decimal. */
  pricePerUnit: string;
  /** Wei escrowed, decimal. */
  budget: string;
  deadline: number;
  requirementsCid: string;
}

export interface OfferTerms {
  state: OfferState;
  jobSpecHash: string;
  requirementsHash: string;
  allowlistRoot: string;
  maxWorkers: number;
  workers: number;
  /** Wei, decimal. */
  joinBond: string;
  coordinator: string;
  disputeWindow: number;
}

export interface Requirements {
  v: number;
  dataClasses: string[];
  architectures: string[];
  minVramGb: number;
  estHours: number;
}

export type RequirementsRead =
  | { status: "notRead"; reason: string }
  | { status: "verified"; requirements: Requirements }
  | { status: "refused"; reason: string };

export type HiddenReason =
  | { reason: "notTakingWorkers" }
  | { reason: "termsNotRead" }
  | { reason: "requirementsNotRead"; detail: string }
  | { reason: "requirementsRefused"; detail: string }
  | { reason: "notYourGroup" }
  | { reason: "posterNotAdmin" };

export interface OfferView {
  opened: OpenedEvent;
  openedBlock: number;
  closed: OfferState | null;
  terms: OfferTerms | null;
  requirements: RequirementsRead;
  hidden: HiddenReason | null;
}

export interface MarketView {
  available: boolean;
  reason: string | null;
  market: string | null;
  offers: OfferView[];
}

/** Ask core for the offers. Rejects outside the desktop app or when core has no such command. */
export function readMarket(): Promise<MarketView> {
  if (BRIDGE_MODE !== "tauri") return Promise.reject(new Error("offers are read by the desktop app"));
  return invoke<MarketView>("fl_market_offers");
}

export const HIDDEN_WORDS: Record<HiddenReason["reason"], string> = {
  notTakingWorkers: "not taking workers (closed, expired or past its deadline)",
  termsNotRead: "the offer's terms have not been read",
  requirementsNotRead: "its requirements file has not been read",
  requirementsRefused: "its requirements file did not match its hash",
  notYourGroup: "a group round for a group you are not in",
  posterNotAdmin: "poster is not an owner or admin on the live roster (FLM-CORE-1)",
};

export function hiddenWords(h: HiddenReason): string {
  const base = HIDDEN_WORDS[h.reason];
  return "detail" in h && h.detail ? `${base}: ${h.detail}` : base;
}

/** What this machine can offer a round, from the hardware probe. */
export interface MachineFacts {
  /** Usable GPU memory in GB (dedicated VRAM, or unified memory on Apple Silicon); null = not read. */
  memoryGb: number | null;
  /** "VRAM" or "unified memory". */
  memoryKind: "VRAM" | "unified memory" | null;
}

export function machineFrom(facts: HardwareFacts | null | undefined): MachineFacts {
  if (!facts) return { memoryGb: null, memoryKind: null };
  if (facts.gpuVramBytes != null && facts.gpuVramBytes > 0) return { memoryGb: Math.floor(facts.gpuVramBytes / 1024 ** 3), memoryKind: "VRAM" };
  if (facts.unifiedMemory && facts.totalRamBytes != null) return { memoryGb: Math.floor(facts.totalRamBytes / 1024 ** 3), memoryKind: "unified memory" };
  return { memoryGb: null, memoryKind: null };
}

/** Decimal SALT ("1.20") to wei, or null when malformed. */
export function weiFromSalt(s: string): bigint | null {
  const m = /^(\d{1,12})(?:\.(\d{1,18}))?$/.exec(s.trim());
  if (!m) return null;
  return BigInt(m[1]) * 10n ** 18n + BigInt((m[2] ?? "").padEnd(18, "0") || "0");
}

export interface Match {
  offer: OfferView;
  joinable: boolean;
  chip: string;
  tone: "ok" | "warn" | "neutral";
  whyLabel: "Why this match:" | "Why not:";
  why: string;
}

/** Match one shown offer against the member's terms and this machine. Pure. */
export function matchOffer(offer: OfferView, terms: WorkerTerms | null, machine: MachineFacts): Match {
  const req = offer.requirements.status === "verified" ? offer.requirements.requirements : null;
  const price = BigInt(offer.opened.pricePerUnit);
  const not: string[] = [];
  const yes: string[] = [];
  if (offer.terms && offer.terms.workers >= offer.terms.maxWorkers) not.push(`all ${offer.terms.maxWorkers} worker places are taken`);
  const floor = terms ? weiFromSalt(terms.floorSaltPerUnit) : null;
  if (floor !== null) {
    if (price < floor) not.push(`below your floor (${saltFromWei(price)} < ${saltFromWei(floor)})`);
    else yes.push("at or above your floor");
  }
  if (req) {
    if (machine.memoryGb === null) not.push("this machine's GPU memory was not read");
    else if (req.minVramGb > machine.memoryGb) not.push(`needs ${req.minVramGb} GB ${machine.memoryKind ?? "VRAM"}; this machine has ${machine.memoryGb} GB`);
    else yes.push(`needs ${req.minVramGb} GB (you have ${machine.memoryGb})`);
    if (terms) {
      const outside = req.dataClasses.filter((c) => !terms.dataClasses.includes(c));
      if (outside.length) not.push(`reads ${outside.join(", ")} data, which your terms do not allow`);
      else if (req.dataClasses.length) yes.push(`reads ${req.dataClasses.join(", ")}, which your terms allow`);
    }
  }
  if (not.length) return { offer, joinable: false, chip: "Not eligible", tone: "warn", whyLabel: "Why not:", why: not.join("; ") + "." };
  if (!terms) return { offer, joinable: true, chip: "No terms set", tone: "neutral", whyLabel: "Why this match:", why: [...yes, "you have not set terms, so only the offer's own rules apply"].join("; ") + "." };
  return { offer, joinable: true, chip: "Inside your terms", tone: "ok", whyLabel: "Why this match:", why: yes.join("; ") + "." };
}

/** Joinable first, then by deadline (soonest first). Never by amount. */
export function rankMatches(ms: Match[]): Match[] {
  return [...ms].sort((a, b) => Number(b.joinable) - Number(a.joinable) || a.offer.opened.deadline - b.offer.opened.deadline);
}

/** Shown offers split from hidden ones (the Advanced table lists both). */
export function splitOffers(view: MarketView): { shown: OfferView[]; hidden: OfferView[] } {
  return { shown: view.offers.filter((o) => o.hidden === null), hidden: view.offers.filter((o) => o.hidden !== null) };
}

/** Offers as CSV for Export (Advanced). */
export function offersCsv(view: MarketView): string {
  const esc = (v: string) => (/[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);
  const rows = [["offer", "poster", "price_per_unit_salt", "deadline", "requirements_cid", "shown", "why_hidden"]];
  for (const o of view.offers) rows.push([String(o.opened.id), o.opened.poster, saltFromWei(BigInt(o.opened.pricePerUnit), 4), String(o.opened.deadline), o.opened.requirementsCid, o.hidden ? "no" : "yes", o.hidden ? hiddenWords(o.hidden) : ""]);
  return rows.map((r) => r.map(esc).join(",")).join("\n") + "\n";
}
