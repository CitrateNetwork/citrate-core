// =====================================================================
// citrate-core: a round draft (FL design board 07, Open a round)
//
// A draft is the four steps' answers, kept on this device until it is signed or discarded. It is
// not a round: nothing is escrowed or published by saving it. Hermes (in a group chat) may hand a
// draft in through `setRoundDraft(draft, "hermes")`; the member then checks every field and signs
// it themselves (HIC-1). Stored in this webview's localStorage; a failed read or write is said,
// never hidden.
// =====================================================================
import { encodeFunctionData, type Hex } from "viem";
import type { FlNetworkState } from "./networkState";

export type RoundTemplate = "tune" | "eval" | "data";
export type WhoMayJoin = "members" | "group" | "anyone";

export interface RoundDraft {
  groupId: string | null;
  template: RoundTemplate;
  dataClass: string;
  /** What a machine needs (the requirements file members' apps check). */
  /** The model architecture the round trains, as a lowercase label (e.g. "llama-8b-lora-r8"). */
  architecture: string;
  minVramGb: number;
  estHours: number;
  /** Decimal SALT, as typed. Never prefilled (G-SEC PAY-19). */
  budgetSalt: string;
  /** Decimal SALT per unit, as typed. Never prefilled. */
  rateSalt: string;
  /** Decimal SALT each worker posts to join, as typed ("0" for none). Never prefilled. */
  joinBondSalt: string;
  /** Decimal SALT the coordinator bonds when it posts the payout; empty = WorkMarket's default. */
  coordinatorBondSalt: string;
  /** Hours members may dispute a posted payout list (WorkMarket's floor is 24). */
  disputeWindowHours: number;
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
  architecture: "",
  minVramGb: 16,
  estHours: 2,
  budgetSalt: "",
  rateSalt: "",
  joinBondSalt: "",
  coordinatorBondSalt: "",
  disputeWindowHours: 24,
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
  saveRoundDraft(handedIn(d, source));
}

/** The money fields of a draft: only the member types these, on the Open a round screen. */
export const MONEY_FIELDS = ["budgetSalt", "rateSalt", "joinBondSalt", "coordinatorBondSalt"] as const;

/** A draft as handed in. G-SEC PAY-19 (owner decision): Hermes may never set money, so a Hermes
 *  draft carries only non-money fields; every money field arrives empty, whatever was passed. */
export function handedIn(d: Partial<RoundDraft>, source: RoundDraft["source"]): RoundDraft {
  const out: RoundDraft = { ...BLANK_DRAFT, ...d, source };
  if (source === "hermes") for (const k of MONEY_FIELDS) out[k] = "";
  return out;
}

export function clearRoundDraft(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* nothing stored */
  }
}

/** Decimal SALT as typed to wei, exactly; null when it is not a SALT amount. */
export function weiOf(s: string): bigint | null {
  const m = /^(\d{1,12})(?:\.(\d{1,18}))?$/.exec(s.trim());
  return m ? BigInt(m[1]) * 10n ** 18n + BigInt((m[2] ?? "").padEnd(18, "0") || "0") : null;
}

/** fl_market.rs label_ok: 1 to 64 of lowercase letters, digits, ".", "_" or "-". */
const LABEL = /^[a-z0-9._-]{1,64}$/;

/** How many leases the budget covers at the rate: floor(budget / rate), exact in wei. */
export function leasesCovered(budgetSalt: string, rateSalt: string): number | null {
  const b = weiOf(budgetSalt);
  const r = weiOf(rateSalt);
  if (b === null || r === null || r === 0n) return null;
  return Number(b / r);
}

/** Why a draft cannot be opened as it stands, or null. */
export function draftProblems(d: RoundDraft): string[] {
  const out: string[] = [];
  if (!d.groupId) out.push("Choose a group");
  if (!d.dataClass.trim()) out.push("Name the data class members train on");
  if (leasesCovered(d.budgetSalt, d.rateSalt) === null) out.push("Enter the budget and the rate as SALT amounts");
  else if ((leasesCovered(d.budgetSalt, d.rateSalt) ?? 0) < 1) out.push("The budget must cover at least one lease");
  if (!(d.maxWorkers >= 3)) out.push("At least three workers (the round ledger's floor)");
  if (!/^\d{4}-\d{2}-\d{2}$/.test(d.deadline)) out.push("Set a deadline");
  if (weiOf(d.joinBondSalt) === null) out.push("Enter the join bond as a SALT amount (0 for none)");
  if (d.coordinatorBondSalt.trim() !== "" && weiOf(d.coordinatorBondSalt) === null) out.push("The coordinator bond must be a SALT amount, or empty for WorkMarket's default");
  if (!(d.disputeWindowHours >= 24)) out.push("The dispute window must be at least 24 hours (WorkMarket's floor)");
  // The offers read model's rule (fl_market.rs requirements_problem): lowercase labels.
  if (d.dataClass.trim() && !LABEL.test(d.dataClass.trim())) out.push("The data class must be a lowercase label (letters, digits, . _ -)");
  if (!LABEL.test(d.architecture.trim())) out.push("Name the architecture as a lowercase label, like llama-8b-lora-r8");
  if (!(Number.isInteger(d.minVramGb) && d.minVramGb >= 0 && d.minVramGb <= 1024)) out.push("Minimum GPU memory must be 0 to 1024 GB");
  if (!(Number.isInteger(d.estHours) && d.estHours >= 1 && d.estHours <= 24 * 90)) out.push("Estimated hours must be 1 to 2160");
  return out;
}

// ---------------------------------------------------------------- WorkMarket.open's parameters

/** WorkMarket.OpenParams (citrate-chain contracts/src/work/WorkMarket.sol), field for field. */
export interface OpenParams {
  kind: 0 | 1;
  verifier: string;
  bindRef: string;
  jobSpecHash: string;
  requirementsHash: string;
  requirementsCid: string;
  groupHash: string;
  allowlistRoot: string;
  pricePerUnit: bigint;
  deadline: bigint;
  disputeWindow: number;
  maxWorkers: number;
  joinBond: bigint;
  coordinator: string;
  coordinatorBond: bigint;
  disputeAdmin: string;
}

/** What the draft cannot say: the round's binding (from the coordinator), its requirements file
 *  (published on IPFS), the verifier and the group. */
export interface OpenBinding {
  verifier: string;
  bindRef: string;
  jobSpecHash: string;
  requirementsHash: string;
  requirementsCid: string;
  groupHash: string;
  coordinator: string;
}

const ZERO_ADDR = "0x" + "0".repeat(40);
const ZERO_32 = "0x" + "0".repeat(64);

/** The exact OpenParams a draft signs (with msg.value = budget), or the problems that stop it. */
export function openParamsOf(d: RoundDraft, b: OpenBinding): { params: OpenParams; valueWei: bigint } | { problems: string[] } {
  const problems = draftProblems(d);
  if (problems.length) return { problems };
  const deadline = BigInt(Math.floor(Date.parse(d.deadline + "T23:59:59Z") / 1000));
  return {
    valueWei: weiOf(d.budgetSalt)!,
    params: {
      kind: 0,
      verifier: b.verifier,
      bindRef: b.bindRef,
      jobSpecHash: b.jobSpecHash,
      requirementsHash: b.requirementsHash,
      requirementsCid: b.requirementsCid,
      groupHash: b.groupHash,
      allowlistRoot: ZERO_32,
      pricePerUnit: weiOf(d.rateSalt)!,
      deadline,
      disputeWindow: d.disputeWindowHours * 3600,
      maxWorkers: d.maxWorkers,
      joinBond: weiOf(d.joinBondSalt)!,
      coordinator: b.coordinator,
      coordinatorBond: d.coordinatorBondSalt.trim() === "" ? 0n : weiOf(d.coordinatorBondSalt)!,
      disputeAdmin: ZERO_ADDR,
    },
  };
}

// ---------------------------------------------------------------- the requirements file (core)

/** The requirements file every member's offers read model checks (fl_market.rs, version 1). */
export interface RequirementsFile {
  v: 1;
  dataClasses: string[];
  architectures: string[];
  minVramGb: number;
  estHours: number;
}

export function requirementsOf(d: RoundDraft): RequirementsFile {
  return { v: 1, dataClasses: [d.dataClass.trim()], architectures: [d.architecture.trim()], minVramGb: d.minVramGb, estHours: d.estHours };
}

export interface PublishedRequirements {
  cid: string;
  requirementsHash: string;
  bytes: number;
}

/** Add and pin the requirements file on the bundled IPFS node (core fl_publish.rs). Its CID and
 *  keccak256 go into OpenParams (requirementsCid, requirementsHash). */
export async function publishRequirements(r: RequirementsFile): Promise<PublishedRequirements> {
  const { BRIDGE_MODE } = await import("../bridge/mode");
  if (BRIDGE_MODE !== "tauri") throw new Error("publishing needs the desktop app's IPFS node");
  const { invoke } = await import("../bridge/tauri/invoke");
  return invoke<PublishedRequirements>("fl_requirements_publish", { requirements: r });
}

// ---------------------------------------------------------------- the open intent (WorkMarket.open)


export const WORK_MARKET_OPEN_ABI = [
  {
    type: "function",
    name: "open",
    stateMutability: "payable",
    inputs: [
      {
        name: "p",
        type: "tuple",
        components: [
          { name: "kind", type: "uint8" },
          { name: "verifier", type: "address" },
          { name: "bindRef", type: "bytes32" },
          { name: "jobSpecHash", type: "bytes32" },
          { name: "requirementsHash", type: "bytes32" },
          { name: "requirementsCid", type: "string" },
          { name: "groupHash", type: "bytes32" },
          { name: "allowlistRoot", type: "bytes32" },
          { name: "pricePerUnit", type: "uint128" },
          { name: "deadline", type: "uint64" },
          { name: "disputeWindow", type: "uint32" },
          { name: "maxWorkers", type: "uint32" },
          { name: "joinBond", type: "uint128" },
          { name: "coordinator", type: "address" },
          { name: "coordinatorBond", type: "uint128" },
          { name: "disputeAdmin", type: "address" },
        ],
      },
    ],
    outputs: [{ name: "id", type: "uint256" }],
  },
] as const;

export const OPEN_SIGNATURE = "open((uint8,address,bytes32,bytes32,bytes32,string,bytes32,bytes32,uint128,uint64,uint32,uint32,uint128,address,uint128,address))";

/** Why a round cannot be opened yet: the coordinator does not publish its round binding. */
export const OPEN_NEEDS_BINDING =
  "opening needs the coordinator's round binding (its on-chain address and FederatedRoundLedger cluster), which the training coordinator does not publish yet";
export const OPEN_WAITS_FOR_GSEC = "Opening escrows SALT, so it opens with payouts after the security review.";

/** The exact WorkMarket.open the wallet review signs (msg.value = the budget), or an Error naming
 *  why there is none. Fails closed: dark payouts, no WorkMarket, no binding, or a bad draft. The
 *  Rust payouts gate refuses the same call at the signer regardless. */
export function openIntent(d: RoundDraft, binding: OpenBinding | null, state: FlNetworkState): { address: string; calldata: string; valueWei: string; label: string } {
  if (state.payouts !== "open") throw new Error(state.payoutsReason ? `${OPEN_WAITS_FOR_GSEC} ${state.payoutsReason}` : OPEN_WAITS_FOR_GSEC);
  const wm = state.contracts.WorkMarket;
  if (wm.kind !== "deployed") throw new Error("WorkMarket is not deployed on this network");
  if (!binding) throw new Error(OPEN_NEEDS_BINDING);
  const r = openParamsOf(d, binding);
  if ("problems" in r) throw new Error(r.problems.join("; "));
  const p = r.params;
  const calldata = encodeFunctionData({
    abi: WORK_MARKET_OPEN_ABI,
    functionName: "open",
    args: [
      {
        kind: p.kind,
        verifier: p.verifier as Hex,
        bindRef: p.bindRef as Hex,
        jobSpecHash: p.jobSpecHash as Hex,
        requirementsHash: p.requirementsHash as Hex,
        requirementsCid: p.requirementsCid,
        groupHash: p.groupHash as Hex,
        allowlistRoot: p.allowlistRoot as Hex,
        pricePerUnit: p.pricePerUnit,
        deadline: p.deadline,
        disputeWindow: p.disputeWindow,
        maxWorkers: p.maxWorkers,
        joinBond: p.joinBond,
        coordinator: p.coordinator as Hex,
        coordinatorBond: p.coordinatorBond,
        disputeAdmin: p.disputeAdmin as Hex,
      },
    ],
  });
  return { address: wm.address, calldata, valueWei: r.valueWei.toString(), label: OPEN_SIGNATURE };
}
