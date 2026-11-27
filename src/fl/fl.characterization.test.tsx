// 0.6.0 L8-WP01 L8-TODAY (characterization, citrate-core a826e441). Registry:
// ci/characterization.d/l8.json; human view: docs/fl/CHARACTERIZATION.md.
//
// Pins what the FL webview derives about this network today, before any L8 change:
//  - PatronageLedger is read with totalUnits() only: one eth_call, the pinned selector, to the
//    book's PatronageLedger (networkState.ts:153-162);
//  - FederatedRoundLedger rounds are inferred from WorkMarket being absent, never read
//    (networkState.ts:163-168);
//  - the phase is "today" without WorkMarket (phaseFor, networkState.ts:116-125);
//  - joinIntent and openIntent refuse first while payouts are not open, in the required words
//    (Matches.tsx:50-53, roundDraft.ts:300-306).
// A failure here means one of those changed. Change an expectation only in the L8 WP the registry
// names for this suite, with a stricter test, in a commit that says why.
//
// Fixtures are test-only (Rule 1): reads are injected fakes, and the one profile that opens
// payouts is built here to show what the dark refusal stands in front of. No shipped profile does.
import { describe, expect, it, vi } from "vitest";
import realBook from "../../src-tauri/addresses/40204.json";
import { ACTIVE_PROFILE, type NetworkProfile } from "../network/profile";
import { bookOnlyState, contractSource, contractWords, DEPLOYED_NO_ROUNDS, readFlNetworkState, TOTAL_UNITS_SELECTOR, type FlBook, type FlReads } from "./networkState";
import { JOIN_SIGNATURE, JOIN_WAITS_FOR_GSEC, joinIntent } from "./Matches";
import type { OfferView } from "./market";
import { BLANK_DRAFT, OPEN_NEEDS_BINDING, OPEN_WAITS_FOR_GSEC, openIntent, type OpenBinding, type RoundDraft } from "./roundDraft";

const PATRONAGE = "0x41f68b9e710585b064459a0ec706148fbf503659";
const FRL = "0x25fc02fbfad9589d0dde84f858d0962bfa0253c5";
const WM = "0x" + "ab".repeat(20);
const ZERO32 = "0x" + "0".repeat(64);
const ONE32 = "0x" + "0".repeat(63) + "1";
const SALT = 10n ** 18n;

const withWm = (extra: Record<string, string | undefined> = {}): FlBook => ({ addresses: { ...realBook.addresses, WorkMarket: WM, ...extra } });
/** Test-only: a profile that opens payouts (no shipped profile does). */
const OPENED = { ...(ACTIVE_PROFILE as NetworkProfile), payoutsOpen: true } as NetworkProfile;

type Answer<T> = T | Error;
/** Injected chain reads that record every call. Code size 1000 and a zero word unless told otherwise. */
function recorder(o: { code?: (a: string) => Answer<number>; view?: (a: string, d: string) => Answer<string>; gate?: () => Answer<{ open: boolean; reason: string | null }> } = {}) {
  const codeCalls: string[] = [];
  const viewCalls: [string, string][] = [];
  const gate = vi.fn(async () => {
    const r = o.gate ? o.gate() : { open: false, reason: null };
    if (r instanceof Error) throw r;
    return r;
  });
  const reads: FlReads = {
    codeSize: async (a) => {
      codeCalls.push(a);
      const r = o.code ? o.code(a) : 1000;
      if (r instanceof Error) throw r;
      return r;
    },
    viewCall: async (a, d) => {
      viewCalls.push([a, d]);
      const r = o.view ? o.view(a, d) : ZERO32;
      if (r instanceof Error) throw r;
      return r;
    },
    ...(o.gate ? { payoutsStatus: gate } : {}),
  };
  return { reads, codeCalls, viewCalls, gate };
}

/** The message a refusal throws, or "no refusal". */
function refusal(f: () => unknown): string {
  try {
    f();
  } catch (e) {
    return e instanceof Error ? e.message : String(e);
  }
  return "no refusal";
}

const offer = (): OfferView => ({
  opened: { id: 15, kind: "flRound", poster: "0x" + "15".repeat(20), groupHash: ZERO32, pricePerUnit: String((3n * SALT) / 2n), budget: String(600n * SALT), deadline: 1_763_769_615, requirementsCid: "bafkreiflm15" },
  openedBlock: 115,
  closed: null,
  terms: { state: "open", jobSpecHash: ZERO32, requirementsHash: ZERO32, allowlistRoot: ZERO32, maxWorkers: 8, workers: 3, joinBond: String(SALT / 10n), coordinator: "0x" + "cd".repeat(20), disputeWindow: 100 },
  requirements: { status: "verified", requirements: { v: 1, dataClasses: ["support-tickets"], architectures: ["llama-8b-lora-r8"], minVramGb: 16, estHours: 2 } },
  hidden: null,
});

const filled: RoundDraft = { ...BLANK_DRAFT, groupId: "0x" + "ab".repeat(16), dataClass: "support-tickets", deadline: "2026-11-22", budgetSalt: "600", rateSalt: "1.50", joinBondSalt: "0.1", architecture: "llama-8b-lora-r8" };
const binding: OpenBinding = { verifier: "0x" + "22".repeat(20), bindRef: "0x" + "3e".repeat(32), jobSpecHash: "0x" + "44".repeat(32), requirementsHash: "0x" + "55".repeat(32), requirementsCid: "bafkreiflm1", groupHash: "0x" + "66".repeat(32), coordinator: "0x" + "cd".repeat(20) };

describe("L8-TODAY: the FL network state and the dark writes as they are at a826e441", () => {
  it("today_patronage_reads_unchanged", async () => {
    // The selector is keccak256("totalUnits()")[0..4], pinned as a literal.
    expect(TOTAL_UNITS_SELECTOR).toBe("0x6d86acc4");
    expect(realBook.addresses.PatronageLedger.toLowerCase()).toBe(PATRONAGE);

    // No units recorded: one eth_call, totalUnits() on PatronageLedger, and nothing else is called.
    const none = recorder();
    const st = await readFlNetworkState(realBook, none.reads);
    expect(none.viewCalls).toEqual([[PATRONAGE, "0x6d86acc4"]]);
    expect(st.contracts.PatronageLedger).toEqual({ kind: "deployed", address: PATRONAGE, codeSize: 1000, rounds: "none" });
    expect(contractWords(st.contracts.PatronageLedger)).toEqual({ text: DEPLOYED_NO_ROUNDS });
    expect(contractSource("PatronageLedger", st.contracts.PatronageLedger)).toBe("eth_getCode + totalUnits()");

    // Units recorded.
    const some = recorder({ view: () => ONE32 });
    const recorded = await readFlNetworkState(realBook, some.reads);
    expect(recorded.contracts.PatronageLedger).toMatchObject({ kind: "deployed", rounds: "recorded" });
    expect(contractWords(recorded.contracts.PatronageLedger)).toEqual({ text: "deployed, rounds recorded" });

    // A malformed answer and a failed read are unknown with the reason, never zero.
    const short = await readFlNetworkState(realBook, recorder({ view: () => "0x" }).reads);
    expect(short.contracts.PatronageLedger).toMatchObject({ rounds: "unknown", roundsReason: "totalUnits() returned no number" });
    const failed = await readFlNetworkState(realBook, recorder({ view: () => new Error("rpc down") }).reads);
    expect(failed.contracts.PatronageLedger).toMatchObject({ rounds: "unknown", roundsReason: "rpc down" });
    expect(contractWords(failed.contracts.PatronageLedger)).toEqual({ text: "deployed", unknown: "rpc down" });

    // With WorkMarket pinned and deployed, PatronageLedger is still the only contract given an eth_call.
    const live = recorder();
    await readFlNetworkState(withWm(), live.reads);
    expect(live.viewCalls).toEqual([[PATRONAGE, "0x6d86acc4"]]);

    // No code at the PatronageLedger pin: no eth_call at all.
    const empty = recorder({ code: (a) => (a === PATRONAGE ? 0 : 1000) });
    const noCode = await readFlNetworkState(realBook, empty.reads);
    expect(empty.viewCalls).toEqual([]);
    expect(noCode.contracts.PatronageLedger).toEqual({ kind: "no-code", address: PATRONAGE });
  });

  it("today_frl_rounds_inferred_from_workmarket", async () => {
    expect(realBook.addresses.FederatedRoundLedger.toLowerCase()).toBe(FRL);

    // No WorkMarket in the book: FRL "no rounds" is inferred, never read, and the tag says so.
    const r = recorder();
    const st = await readFlNetworkState(realBook, r.reads);
    expect(st.contracts.FederatedRoundLedger).toEqual({ kind: "deployed", address: FRL, codeSize: 1000, rounds: "none" });
    expect(r.viewCalls.some(([a]) => a === FRL)).toBe(false);
    expect(contractWords(st.contracts.FederatedRoundLedger)).toEqual({ text: DEPLOYED_NO_ROUNDS });
    expect(contractSource("FederatedRoundLedger", st.contracts.FederatedRoundLedger)).toBe("eth_getCode + no WorkMarket in the address book");

    // WorkMarket deployed: FRL rounds are unknown here (read per offer), still never read.
    const lr = recorder();
    const live = await readFlNetworkState(withWm(), lr.reads);
    expect(live.contracts.FederatedRoundLedger).toEqual({ kind: "deployed", address: FRL, codeSize: 1000, rounds: "unknown", roundsReason: "rounds are read per offer" });
    expect(lr.viewCalls.some(([a]) => a === FRL)).toBe(false);
    expect(contractWords(live.contracts.FederatedRoundLedger)).toEqual({ text: "deployed", unknown: "rounds are read per offer" });
    expect(contractSource("FederatedRoundLedger", live.contracts.FederatedRoundLedger)).toBe("eth_getCode");

    // WorkMarket pinned but with no code counts as absent: FRL "no rounds" again.
    const pinnedNoCode = await readFlNetworkState(withWm(), recorder({ code: (a) => (a === WM ? 0 : 1000) }).reads);
    expect(pinnedNoCode.contracts.FederatedRoundLedger).toMatchObject({ kind: "deployed", rounds: "none" });

    // The other ledgers carry no round reading.
    expect(st.contracts.LoRAFactory).toMatchObject({ kind: "deployed", rounds: "not-applicable" });
    expect(contractWords(st.contracts.LoRAFactory)).toEqual({ text: "deployed" });
  });

  it("today_phase_today_without_workmarket", async () => {
    const label = ACTIVE_PROFILE?.label ?? "Network not set";

    // Book only (no chain read yet): WorkMarket, FLRoundVerifier and DatasetRegistry are not in the
    // shipped book, so the phase is already "today" and payouts "not-deployed".
    const book = bookOnlyState(realBook);
    expect(book).toMatchObject({ phase: "today", payouts: "not-deployed", networkLabel: label });
    expect(book.contracts.WorkMarket).toEqual({ kind: "not-in-book" });
    expect(book.contracts.FLRoundVerifier).toEqual({ kind: "not-in-book" });
    expect(book.contracts.DatasetRegistry).toEqual({ kind: "not-in-book" });
    expect(book.contracts.FederatedRoundLedger).toEqual({ kind: "reading", address: FRL });

    // After the reads: still "today", no reason, and the payouts gate is never asked.
    const r = recorder({ gate: () => ({ open: true, reason: null }) });
    const st = await readFlNetworkState(realBook, r.reads);
    expect(st.phase).toBe("today");
    expect(st.phaseReason).toBeUndefined();
    expect(st.payouts).toBe("not-deployed");
    expect(r.gate).not.toHaveBeenCalled();
    expect(r.codeCalls).not.toContain(WM);

    // A zero or malformed WorkMarket pin is no pin.
    expect((await readFlNetworkState(withWm({ WorkMarket: "0x" + "0".repeat(40) }), recorder().reads)).phase).toBe("today");
    expect((await readFlNetworkState(withWm({ WorkMarket: "0x1234" }), recorder().reads)).phase).toBe("today");

    // WorkMarket pinned with no code: "today", payouts not deployed.
    const noCode = await readFlNetworkState(withWm(), recorder({ code: (a) => (a === WM ? 0 : 1000) }).reads);
    expect(noCode).toMatchObject({ phase: "today", payouts: "not-deployed" });

    // The WorkMarket code read failed: unknown with its reason, never "today".
    const wmDown = await readFlNetworkState(withWm(), recorder({ code: (a) => (a === WM ? new Error("boom") : 1000) }).reads);
    expect(wmDown).toMatchObject({ phase: "unknown", phaseReason: "WorkMarket: boom", payouts: "unknown" });

    // WorkMarket pinned, reads in flight: "reading", payouts dark.
    expect(bookOnlyState(withWm())).toMatchObject({ phase: "reading", payouts: "dark" });

    // WorkMarket deployed: the FRL reading decides.
    const frlDown = await readFlNetworkState(withWm(), recorder({ code: (a) => (a === FRL ? new Error("frl down") : 1000) }).reads);
    expect(frlDown).toMatchObject({ phase: "unknown", phaseReason: "FederatedRoundLedger: frl down" });
    expect((await readFlNetworkState(withWm(), recorder({ code: (a) => (a === FRL ? 0 : 1000) }).reads)).phase).toBe("today");
    expect((await readFlNetworkState(withWm({ FederatedRoundLedger: undefined }), recorder().reads)).phase).toBe("today");

    // Both deployed: "rounds-live", and payouts stay dark on the shipped profile; core's gate
    // decides, asked once, and its reason is kept.
    const lr = recorder({ gate: () => ({ open: false, reason: "payouts are not open on r1007" }) });
    const live = await readFlNetworkState(withWm(), lr.reads);
    expect(live).toMatchObject({ phase: "rounds-live", payouts: "dark", payoutsReason: "payouts are not open on r1007" });
    expect(lr.gate).toHaveBeenCalledTimes(1);
    expect((await readFlNetworkState(withWm(), recorder().reads)).payouts).toBe("dark");
  });

  it("today_join_refused_while_dark", async () => {
    expect(JOIN_WAITS_FOR_GSEC).toBe("Joining posts a SALT join bond, so it opens with payouts after the security review.");

    // Today (no WorkMarket): refused with the required words, before the offer is looked at.
    const today = await readFlNetworkState(realBook, recorder().reads);
    expect(refusal(() => joinIntent(offer(), today))).toBe(JOIN_WAITS_FOR_GSEC);
    expect(refusal(() => joinIntent({ ...offer(), terms: null }, today))).toBe(JOIN_WAITS_FOR_GSEC);

    // WorkMarket deployed, payouts dark: the same words; core's reason is not appended to a join.
    const dark = await readFlNetworkState(withWm(), recorder({ gate: () => ({ open: false, reason: "payouts are not open on r1007" }) }).reads);
    expect(dark.payouts).toBe("dark");
    expect(refusal(() => joinIntent(offer(), dark))).toBe(JOIN_WAITS_FOR_GSEC);
    const unread = await readFlNetworkState(withWm(), recorder({ gate: () => new Error("ipc down") }).reads);
    expect(unread).toMatchObject({ payouts: "dark", payoutsReason: "the payouts gate was not read: ipc down" });
    expect(refusal(() => joinIntent(offer(), unread))).toBe(JOIN_WAITS_FOR_GSEC);

    // Only an open state (test-only profile) builds the join: the dark check is what refuses above.
    const opened = await readFlNetworkState(withWm(), recorder().reads, OPENED);
    expect(joinIntent(offer(), opened)).toMatchObject({ address: WM, valueWei: String(SALT / 10n), label: JOIN_SIGNATURE });
  });

  it("today_open_refused_while_dark", async () => {
    expect(OPEN_WAITS_FOR_GSEC).toBe("Opening escrows SALT, so it opens with payouts after the security review.");

    // Today (no WorkMarket): refused first, ahead of the missing binding and a blank draft.
    const today = await readFlNetworkState(realBook, recorder().reads);
    expect(refusal(() => openIntent(filled, binding, today))).toBe(OPEN_WAITS_FOR_GSEC);
    expect(refusal(() => openIntent(filled, null, today))).toBe(OPEN_WAITS_FOR_GSEC);
    expect(refusal(() => openIntent(BLANK_DRAFT, binding, today))).toBe(OPEN_WAITS_FOR_GSEC);

    // WorkMarket deployed, payouts dark: the words, then core's reason.
    const dark = await readFlNetworkState(withWm(), recorder({ gate: () => ({ open: false, reason: "payouts are not open on r1007" }) }).reads);
    expect(refusal(() => openIntent(filled, binding, dark))).toBe(`${OPEN_WAITS_FOR_GSEC} payouts are not open on r1007`);
    const unread = await readFlNetworkState(withWm(), recorder({ gate: () => new Error("ipc down") }).reads);
    expect(refusal(() => openIntent(filled, binding, unread))).toBe(`${OPEN_WAITS_FOR_GSEC} the payouts gate was not read: ipc down`);
    const noReason = await readFlNetworkState(withWm(), recorder().reads);
    expect(refusal(() => openIntent(filled, binding, noReason))).toBe(OPEN_WAITS_FOR_GSEC);

    // Only once payouts are open does the next check (the coordinator's binding) speak.
    const opened = await readFlNetworkState(withWm(), recorder().reads, OPENED);
    expect(refusal(() => openIntent(filled, null, opened))).toBe(OPEN_NEEDS_BINDING);
  });
});
