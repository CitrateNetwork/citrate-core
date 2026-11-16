// =====================================================================
// citrate-core: the FL network state (0.5.4 FL design, Q1 and Q8)
//
// What the FL views may say about this network, derived ONLY from the shipped address book and
// chain reads. There is no user-facing network switch: the "Today (0.5.4)" state and the 0.6.0
// states are both read from the data, and the 0.6.0 states render only when the data shows them.
//
// Data sources (Rule 7):
//  - the generated address book `src-tauri/addresses/40204.json` for each contract's pin;
//  - `eth_getCode` on the active network (core's `contract_code_size`) for each pinned address;
//  - `eth_call PatronageLedger.totalUnits()` (core's `contract_view_call`): no units recorded
//    means no round has been recorded on the cooperative ledger;
//  - the network profile (src-tauri/network/profiles.json) for whether payouts are open (G-SEC).
//
// On r1007 the book pins FederatedRoundLedger, PatronageLedger and LoRAFactory (block 0) and does
// not pin WorkMarket, FLRoundVerifier or DatasetRegistry, so the state is "today": group rounds
// not live yet, the three ledgers "deployed, no rounds" / "deployed", the rest "Not available yet
// on this network". A failed read is "unknown" with its reason, never zero and never "not
// deployed".
// =====================================================================
import { toFunctionSelector } from "viem";
import { ACTIVE_PROFILE, type NetworkProfile } from "../network/profile";

export const FL_CONTRACTS = ["FederatedRoundLedger", "PatronageLedger", "LoRAFactory", "WorkMarket", "FLRoundVerifier", "DatasetRegistry"] as const;
export type FlContractName = (typeof FL_CONTRACTS)[number];

/** The contracts the hub's contract table lists (board 05, Today). */
export const HUB_CONTRACTS: FlContractName[] = ["FederatedRoundLedger", "PatronageLedger", "LoRAFactory", "WorkMarket"];

/** The required wordings (UI truth contract v1.2 §1; design Q6). */
export const ROUNDS_NOT_LIVE = "Group rounds not live yet";
export const NOT_AVAILABLE = "Not available yet on this network";
export const DEPLOYED_NO_ROUNDS = "deployed, no rounds";
export const PAYOUTS_DARK = "Payouts open after the security review";
export const NO_TRAINING_YET = "No training starts from the app yet";

export interface FlBook {
  addresses: Record<string, string | undefined>;
}

export type ContractReading =
  /** The book has no usable pin for this contract. */
  | { kind: "not-in-book" }
  /** Pinned; the code read has not answered yet. */
  | { kind: "reading"; address: string }
  /** Pinned, but there is no code at the address. */
  | { kind: "no-code"; address: string }
  /** The read failed: unknown, never zero. */
  | { kind: "unknown"; address: string; reason: string }
  /** Code at the pinned address. `rounds` is what the ledger reads say about recorded rounds. */
  | { kind: "deployed"; address: string; codeSize: number; rounds: "none" | "recorded" | "not-applicable" | "unknown"; roundsReason?: string };

export type FlPhase =
  /** Reads in flight; the view shows the book-only state. */
  | "reading"
  /** WorkMarket is not on this network: group rounds not live yet (0.5.4). */
  | "today"
  /** WorkMarket and FederatedRoundLedger are deployed: rounds can be offered (0.6.0). */
  | "rounds-live"
  /** A read the phase depends on failed. */
  | "unknown";

export type PayoutsState = "not-deployed" | "dark" | "open" | "unknown";

export interface FlNetworkState {
  contracts: Record<FlContractName, ContractReading>;
  phase: FlPhase;
  /** Why the phase is unknown, when it is. */
  phaseReason?: string;
  payouts: PayoutsState;
  /** Network label from the active profile. */
  networkLabel: string;
}

export interface FlReads {
  codeSize: (address: string) => Promise<number>;
  viewCall: (address: string, calldata: string) => Promise<string>;
}

const ADDRESS = /^0x[0-9a-f]{40}$/;
const ZERO = "0x" + "0".repeat(40);

/** The book's pin for a contract, lowercase, or null when it has none (absent, malformed, zero). */
export function pinOf(book: FlBook, name: FlContractName): string | null {
  const raw = (book.addresses[name] ?? "").trim().toLowerCase();
  return ADDRESS.test(raw) && raw !== ZERO ? raw : null;
}

/** Payouts open only when the network profile says so (G-SEC). No shipped profile does. */
export function payoutsEnabledBy(profile: NetworkProfile | null): boolean {
  return profile?.payoutsOpen === true;
}

function payoutsFor(work: ContractReading, profile: NetworkProfile | null): PayoutsState {
  if (work.kind === "not-in-book" || work.kind === "no-code") return "not-deployed";
  if (work.kind === "unknown") return "unknown";
  if (work.kind === "reading") return "dark";
  return payoutsEnabledBy(profile) ? "open" : "dark";
}

function phaseFor(c: Record<FlContractName, ContractReading>): { phase: FlPhase; reason?: string } {
  const wm = c.WorkMarket;
  const frl = c.FederatedRoundLedger;
  if (wm.kind === "not-in-book" || wm.kind === "no-code") return { phase: "today" };
  if (wm.kind === "unknown") return { phase: "unknown", reason: `WorkMarket: ${wm.reason}` };
  if (wm.kind === "reading" || frl.kind === "reading") return { phase: "reading" };
  if (frl.kind === "unknown") return { phase: "unknown", reason: `FederatedRoundLedger: ${frl.reason}` };
  if (frl.kind !== "deployed") return { phase: "today" };
  return { phase: "rounds-live" };
}

function assemble(contracts: Record<FlContractName, ContractReading>, profile: NetworkProfile | null): FlNetworkState {
  const p = phaseFor(contracts);
  return {
    contracts,
    phase: p.phase,
    ...(p.reason ? { phaseReason: p.reason } : {}),
    payouts: payoutsFor(contracts.WorkMarket, profile),
    networkLabel: profile?.label ?? "Network not set",
  };
}

/** The state available without touching the chain: unpinned contracts are known not to be here. */
export function bookOnlyState(book: FlBook, profile: NetworkProfile | null = ACTIVE_PROFILE): FlNetworkState {
  const contracts = {} as Record<FlContractName, ContractReading>;
  for (const n of FL_CONTRACTS) {
    const a = pinOf(book, n);
    contracts[n] = a === null ? { kind: "not-in-book" } : { kind: "reading", address: a };
  }
  return assemble(contracts, profile);
}

/** PatronageLedger.totalUnits() */
export const TOTAL_UNITS_SELECTOR = toFunctionSelector("totalUnits()");

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

async function roundsOn(name: FlContractName, address: string, reads: FlReads, workMarketHere: boolean): Promise<Pick<Extract<ContractReading, { kind: "deployed" }>, "rounds" | "roundsReason">> {
  if (name === "PatronageLedger") {
    try {
      const out = await reads.viewCall(address, TOTAL_UNITS_SELECTOR);
      if (!/^0x[0-9a-fA-F]{64}$/.test(out)) return { rounds: "unknown", roundsReason: "totalUnits() returned no number" };
      return BigInt(out) === 0n ? { rounds: "none" } : { rounds: "recorded" };
    } catch (e) {
      return { rounds: "unknown", roundsReason: msg(e) };
    }
  }
  if (name === "FederatedRoundLedger") {
    // FRL keeps rounds in an internal mapping with no counter, so the app cannot list them. Rounds
    // reach members only through WorkMarket offers; with no WorkMarket on this network there is no
    // round for a member to take part in.
    return workMarketHere ? { rounds: "unknown", roundsReason: "rounds are read per offer" } : { rounds: "none" };
  }
  return { rounds: "not-applicable" };
}

/** Read every pinned contract's code (and the ledger round reads) and derive the state. */
export async function readFlNetworkState(book: FlBook, reads: FlReads, profile: NetworkProfile | null = ACTIVE_PROFILE): Promise<FlNetworkState> {
  const first = bookOnlyState(book, profile);
  const contracts = { ...first.contracts };
  await Promise.all(
    FL_CONTRACTS.map(async (n) => {
      const c = contracts[n];
      if (c.kind !== "reading") return;
      try {
        const size = await reads.codeSize(c.address);
        contracts[n] = size > 0 ? { kind: "deployed", address: c.address, codeSize: size, rounds: "not-applicable" } : { kind: "no-code", address: c.address };
      } catch (e) {
        contracts[n] = { kind: "unknown", address: c.address, reason: msg(e) };
      }
    }),
  );
  const workMarketHere = contracts.WorkMarket.kind === "deployed";
  await Promise.all(
    FL_CONTRACTS.map(async (n) => {
      const c = contracts[n];
      if (c.kind !== "deployed") return;
      contracts[n] = { ...c, ...(await roundsOn(n, c.address, reads, workMarketHere)) };
    }),
  );
  return assemble(contracts, profile);
}

/** The row value the contract table shows, in the required words. */
export function contractWords(c: ContractReading): { text: string; unknown?: string } {
  switch (c.kind) {
    case "not-in-book":
    case "no-code":
      return { text: NOT_AVAILABLE };
    case "reading":
      return { text: "Reading" };
    case "unknown":
      return { text: "—", unknown: c.reason };
    case "deployed":
      if (c.rounds === "none") return { text: DEPLOYED_NO_ROUNDS };
      if (c.rounds === "recorded") return { text: "deployed, rounds recorded" };
      if (c.rounds === "unknown") return { text: "deployed", unknown: c.roundsReason ?? "rounds not read" };
      return { text: "deployed" };
  }
}

/** Where a contract row's value comes from, for its source tag. */
export function contractSource(name: FlContractName, c: ContractReading): string {
  if (c.kind === "not-in-book") return "address book";
  if (c.kind === "deployed" && name === "PatronageLedger" && c.rounds !== "not-applicable") return "eth_getCode + totalUnits()";
  // FRL has no round counter: its "no rounds" is inferred from WorkMarket missing from the book,
  // the only path that offers a round to members. The tag says so.
  if (c.kind === "deployed" && name === "FederatedRoundLedger" && c.rounds === "none") return "eth_getCode + no WorkMarket in the address book";
  return "eth_getCode";
}
