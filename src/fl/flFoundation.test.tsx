// FL design foundation (0.5.4): the network state model, the shared FL primitives, the ceremony
// sheet's truth-contract words, authority and worker terms, and the Groups › Rounds sub-tabs.
// Fixtures here are test-only: no production module builds a state from them (Rule 1).
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { act } from "react";
import { createRoot } from "react-dom/client";
import realBook from "../../src-tauri/addresses/40204.json";
import { ACTIVE_PROFILE, type NetworkProfile } from "../network/profile";
import {
  bookOnlyState,
  contractSource,
  contractWords,
  DEPLOYED_NO_ROUNDS,
  NOT_AVAILABLE,
  payoutsEnabledBy,
  pinOf,
  readFlNetworkState,
  ROUNDS_NOT_LIVE,
  TOTAL_UNITS_SELECTOR,
  type FlBook,
  type FlReads,
} from "./networkState";
import { RoundRing, ringPaint, RING_STAGES } from "./RoundRing";
import { LeaseDots, leaseSentence, orderLeases, type Lease } from "./LeaseDots";
import { ChallengeClock, clockFraction, clockSentence } from "./ChallengeClock";
import { BelnapCounts, belnapSentence } from "./BelnapCounts";
import { EscrowBar, escrowSentence } from "./EscrowBar";
import { ContributionTimeline } from "./ContributionTimeline";
import { PayoutReceipt, payoutSentence, type PayoutStatus } from "./PayoutReceipt";
import { CeremonySheet, outcomeFromBroadcastError, outcomeFromReceipt, outcomeWords } from "./CeremonySheet";
import { Chip, HicBadge, KV, Unknown, shortHex } from "./primitives";
import { aboutDuration, saltFromWei, saltText } from "./format";
import { HIC1, selectAuthority, setEnvelopeForAuthority, subscribeAuthority } from "./authority";
import { loadWorkerTerms, saveWorkerTerms, TERMS_SAVE_UNAVAILABLE, termsProblem, type WorkerTerms } from "./workerTerms";
import { RoundsNotLive } from "./RoundsNotLive";
import { ContractTable } from "./ContractTable";
import { FlBand } from "./FlBand";
import { CANONICAL_ROUTES, locate, registerFor, resolveRoute } from "../shell/places";

const WM = "0x" + "ab".repeat(20);
const ONE = "0x" + "0".repeat(63) + "1";
const ZERO_WORD = "0x" + "0".repeat(64);
const SALT = 10n ** 18n;

const reads = (over: Partial<FlReads> = {}): FlReads => ({
  codeSize: vi.fn(async () => 1000),
  viewCall: vi.fn(async () => ZERO_WORD),
  ...over,
});

const withWorkMarket: FlBook = { addresses: { ...realBook.addresses, WorkMarket: WM } };

/** Words the FL surfaces must never render (copy deck). */
const BANNED = /\b(earn|earns|earned|earning|earnings|income|projected|estimate|yield|APY|APR|profit|investment|USD|dormant|Final|Settled|HITL|users?)\b|!|\$/;

describe("FL network state: today's book (r1007)", () => {
  it("pins the three block-0 ledgers and not WorkMarket, FLRoundVerifier or DatasetRegistry", () => {
    expect(pinOf(realBook, "FederatedRoundLedger")).toBe("0x25fc02fbfad9589d0dde84f858d0962bfa0253c5");
    expect(pinOf(realBook, "PatronageLedger")).toBe("0x41f68b9e710585b064459a0ec706148fbf503659");
    expect(pinOf(realBook, "LoRAFactory")).toBe("0x62e9aabec26288e60db2503f518eacf9cdcc8890");
    for (const n of ["WorkMarket", "FLRoundVerifier", "DatasetRegistry"] as const) expect(pinOf(realBook, n)).toBeNull();
  });

  it("is 'today' from the book alone, before any chain read", () => {
    const s = bookOnlyState(realBook);
    expect(s.phase).toBe("today");
    expect(s.payouts).toBe("not-deployed");
    expect(s.contracts.WorkMarket.kind).toBe("not-in-book");
    expect(s.contracts.FederatedRoundLedger.kind).toBe("reading");
  });

  it("reads code at the pinned addresses and totalUnits() on PatronageLedger only", async () => {
    const r = reads();
    const s = await readFlNetworkState(realBook, r);
    expect((r.codeSize as ReturnType<typeof vi.fn>).mock.calls.map((c) => c[0]).sort()).toEqual(
      ["0x25fc02fbfad9589d0dde84f858d0962bfa0253c5", "0x41f68b9e710585b064459a0ec706148fbf503659", "0x62e9aabec26288e60db2503f518eacf9cdcc8890"].sort(),
    );
    expect(r.viewCall).toHaveBeenCalledTimes(1);
    expect(r.viewCall).toHaveBeenCalledWith("0x41f68b9e710585b064459a0ec706148fbf503659", TOTAL_UNITS_SELECTOR);
    expect(TOTAL_UNITS_SELECTOR).toMatch(/^0x[0-9a-f]{8}$/);
    expect(s.phase).toBe("today");
    expect(contractWords(s.contracts.FederatedRoundLedger).text).toBe(DEPLOYED_NO_ROUNDS);
    expect(contractWords(s.contracts.PatronageLedger).text).toBe(DEPLOYED_NO_ROUNDS);
    expect(contractWords(s.contracts.LoRAFactory).text).toBe("deployed");
    expect(contractWords(s.contracts.WorkMarket).text).toBe(NOT_AVAILABLE);
    expect(contractWords(s.contracts.DatasetRegistry).text).toBe(NOT_AVAILABLE);
    expect(contractSource("WorkMarket", s.contracts.WorkMarket)).toBe("address book");
    expect(contractSource("PatronageLedger", s.contracts.PatronageLedger)).toBe("eth_getCode + totalUnits()");
    // #685 review: FRL's "no rounds" is labelled with what it is inferred from.
    expect(contractSource("FederatedRoundLedger", s.contracts.FederatedRoundLedger)).toBe("eth_getCode + no WorkMarket in the address book");
    expect(contractSource("LoRAFactory", s.contracts.LoRAFactory)).toBe("eth_getCode");
  });

  it("a failed code read is unknown with its reason, never zero and never 'not available'", async () => {
    const s = await readFlNetworkState(realBook, reads({ codeSize: async () => Promise.reject(new Error("rpc down")) }));
    const w = contractWords(s.contracts.FederatedRoundLedger);
    expect(w).toEqual({ text: "—", unknown: "rpc down" });
    expect(s.phase).toBe("today");
  });

  it("pinned with no code is 'Not available yet on this network'", async () => {
    const s = await readFlNetworkState(realBook, reads({ codeSize: async () => 0 }));
    expect(s.contracts.LoRAFactory.kind).toBe("no-code");
    expect(contractWords(s.contracts.LoRAFactory).text).toBe(NOT_AVAILABLE);
  });

  it("PatronageLedger with units recorded, or a failed units read, is not 'no rounds'", async () => {
    const rec = await readFlNetworkState(realBook, reads({ viewCall: async () => ONE }));
    expect(contractWords(rec.contracts.PatronageLedger).text).toBe("deployed, rounds recorded");
    const bad = await readFlNetworkState(realBook, reads({ viewCall: async () => Promise.reject(new Error("call reverted")) }));
    expect(contractWords(bad.contracts.PatronageLedger)).toEqual({ text: "deployed", unknown: "call reverted" });
    const junk = await readFlNetworkState(realBook, reads({ viewCall: async () => "0x" }));
    expect(contractWords(junk.contracts.PatronageLedger).unknown).toMatch(/no number/);
  });
});

describe("FL network state: 0.6.0 states render only when the data shows them", () => {
  it("WorkMarket pinned with code: rounds live, payouts dark until the profile opens them", async () => {
    const s = await readFlNetworkState(withWorkMarket, reads());
    expect(s.phase).toBe("rounds-live");
    expect(s.payouts).toBe("dark");
    expect(contractWords(s.contracts.FederatedRoundLedger).text).toBe("deployed");
  });
  it("payouts open only on a profile that says payoutsOpen: true (no shipped profile does)", async () => {
    const open = { ...(ACTIVE_PROFILE as NetworkProfile), payoutsOpen: true } as NetworkProfile;
    expect(payoutsEnabledBy(ACTIVE_PROFILE)).toBe(false);
    expect(payoutsEnabledBy(open)).toBe(true);
    const s = await readFlNetworkState(withWorkMarket, reads(), open);
    expect(s.payouts).toBe("open");
  });
  it("core's payouts gate (fl_payouts_status) is authoritative over the profile flag", async () => {
    const flagged = { ...(ACTIVE_PROFILE as NetworkProfile), payoutsOpen: true } as NetworkProfile;
    const shut = await readFlNetworkState(withWorkMarket, reads({ payoutsStatus: async () => ({ open: false, reason: "WorkMarket's code does not match its pin" }) }), flagged);
    expect(shut.payouts).toBe("dark");
    expect(shut.payoutsReason).toBe("WorkMarket's code does not match its pin");
    const opened = await readFlNetworkState(withWorkMarket, reads({ payoutsStatus: async () => ({ open: true, reason: null }) }));
    expect(opened.payouts).toBe("open");
    const unread = await readFlNetworkState(withWorkMarket, reads({ payoutsStatus: async () => Promise.reject(new Error("no such command")) }), flagged);
    expect(unread).toMatchObject({ payouts: "dark", payoutsReason: "the payouts gate was not read: no such command" });
    const g = vi.fn(async () => ({ open: true, reason: null }));
    const r1007 = await readFlNetworkState(realBook, reads({ payoutsStatus: g }));
    expect(r1007.payouts).toBe("not-deployed");
    expect(g).not.toHaveBeenCalled();
  });
  it("a failed WorkMarket read is phase unknown with the reason", async () => {
    const s = await readFlNetworkState(withWorkMarket, reads({ codeSize: async (a) => (a === WM ? Promise.reject(new Error("timeout")) : 10) }));
    expect(s.phase).toBe("unknown");
    expect(s.phaseReason).toContain("timeout");
    expect(s.payouts).toBe("unknown");
  });
  it("zero and malformed pins count as absent", () => {
    expect(pinOf({ addresses: { WorkMarket: "0x" + "0".repeat(40) } }, "WorkMarket")).toBeNull();
    expect(pinOf({ addresses: { WorkMarket: "0x1234" } }, "WorkMarket")).toBeNull();
    expect(pinOf({ addresses: { WorkMarket: WM.toUpperCase().replace("0X", "0x") } }, "WorkMarket")).toBe(WM);
  });
});

describe("Rounds Today view", () => {
  it("shows the required words, the contract table and the real stage, with no banned copy", async () => {
    const state = await readFlNetworkState(realBook, reads());
    const h = renderToStaticMarkup(<RoundsNotLive state={state} headline="Rounds open on this network after the WorkMarket contract is deployed." />);
    expect(h).toContain(ROUNDS_NOT_LIVE);
    expect(h).toContain(DEPLOYED_NO_ROUNDS);
    expect(h).toContain(NOT_AVAILABLE);
    expect(h).toContain("No training starts from the app yet");
    const el = document.createElement("div");
    el.innerHTML = h;
    expect(el.textContent ?? "").not.toMatch(BANNED);
  });
  it("phase unknown says so with the reason", async () => {
    const state = await readFlNetworkState(withWorkMarket, reads({ codeSize: async () => Promise.reject(new Error("rpc down")) }));
    const h = renderToStaticMarkup(<RoundsNotLive state={state} headline="x" />);
    expect(h).toContain("Round contracts: —");
    expect(h).toContain("rpc down");
  });
  it("the contract table names each row's source", async () => {
    const state = await readFlNetworkState(realBook, reads());
    const h = renderToStaticMarkup(<ContractTable state={state} />);
    expect(h).toContain("address book");
    expect(h).toContain("eth_getCode");
    expect((h.match(/data-contract=/g) ?? []).length).toBe(4);
  });
});

describe("FL header band", () => {
  const base = { liquid: 4210.5, netUnread: [], node: "synced" as const, height: 1204311 };
  it("shows the wallet balance once read and the node height", () => {
    const h = renderToStaticMarkup(<FlBand s={base} networkLabel="Citrate testnet" />);
    expect(h).toContain("4,210.50 SALT");
    expect(h).toContain("block 1,204,311");
    expect(h).toContain("Not bundled in this release");
  });
  it("an unread balance is —, never 0", () => {
    const h = renderToStaticMarkup(<FlBand s={{ ...base, liquid: 0, netUnread: ["balances"] }} networkLabel="n" />);
    expect(h).not.toContain("0.00 SALT");
    expect(h).toContain("not read yet on this network");
  });
});

describe("Round ring", () => {
  it("paints passed, current and track segments", () => {
    expect(ringPaint("verifying")).toEqual(["done", "done", "current", "track", "track"]);
    expect(ringPaint("open")).toEqual(["current", "track", "track", "track", "track"]);
    expect(ringPaint("closed")).toEqual(["done", "done", "done", "done", "current"]);
  });
  it("disputed marks Payout, rejected marks Verifying, expired greys every segment", () => {
    expect(ringPaint("payout", "disputed")[3]).toBe("disputed");
    expect(ringPaint("verifying", "rejected")[2]).toBe("rejected");
    expect(new Set(ringPaint("running", "expired"))).toEqual(new Set(["expired"]));
  });
  it("is one labelled image, the current segment wider, and the legend names every stage", () => {
    const label = "Round 14: verifying, 6 of 8 leases verified";
    const h = renderToStaticMarkup(<RoundRing stage="verifying" label={label} legend />);
    expect(h).toContain(`role="img"`);
    expect(h).toContain(`aria-label="${label}"`);
    for (const w of ["Open", "Running", "Verifying", "Payout", "Closed"]) expect(h).toContain(w);
    const widths = [...h.matchAll(/data-paint="(\w+)"[^>]*stroke-width="([\d.]+)"/g)].map((m) => [m[1], Number(m[2])] as const);
    expect(widths.length).toBe(RING_STAGES.length);
    const cur = widths.find((w) => w[0] === "current")![1];
    for (const w of widths.filter((x) => x[0] !== "current")) expect(cur).toBeGreaterThan(w[1]);
  });
});

describe("Lease dots", () => {
  const leases: Lease[] = [
    { id: "3", state: "verified" },
    { id: "7", state: "rejected", reason: "the model update failed the file format check" },
    { id: "9", state: "leased", mine: true },
    { id: "2", state: "submitted" },
  ];
  it("puts your lease first and says each lease in words", () => {
    expect(orderLeases(leases).map((l) => l.id)).toEqual(["9", "3", "7", "2"]);
    expect(leaseSentence(leases[1])).toBe("Lease 7: rejected, the model update failed the file format check");
    expect(leaseSentence(leases[2])).toBe("Your lease 9: leased");
  });
  it("differs by glyph, not colour alone, and each lease is a real button", () => {
    const h = renderToStaticMarkup(<LeaseDots leases={leases} onSelect={() => {}} selected="7" />);
    for (const g of ["○", "●", "✓", "⊘"]) expect(h).toContain(g);
    expect((h.match(/<button/g) ?? []).length).toBe(4);
    expect(h).toContain(`aria-pressed="true"`);
    expect(h).toContain("4 leases: 1 verified, 1 submitted, 1 leased, 1 rejected");
  });
});

describe("Challenge clock", () => {
  const open = { state: "open" as const, commitBlock: 1000, endBlock: 2000, head: 1500, blockSeconds: 2 };
  it("says the end block and a duration only from a measured block time", () => {
    expect(clockSentence(open)).toBe("Challenge window ends at block 2,000, in about 17 min");
    expect(clockSentence({ ...open, blockSeconds: null })).toBe("Challenge window ends at block 2,000, 500 blocks from now");
    expect(clockSentence({ ...open, head: null })).toContain("current block was not read");
    expect(clockFraction(open)).toBe(0.5);
    expect(clockFraction({ ...open, head: null })).toBeNull();
  });
  it("names closed, ended early and not open", () => {
    expect(clockSentence({ ...open, state: "closed" })).toContain("closed at block 2,000");
    expect(clockSentence({ ...open, state: "ended-early" })).toContain("a challenge was proven");
    expect(clockSentence({ state: "not-open", head: null, blockSeconds: null })).toContain("opens when the merge is committed");
    expect(renderToStaticMarkup(<ChallengeClock {...open} />)).toContain(`role="img"`);
  });
});

describe("Belnap counts, escrow bar, contribution timeline", () => {
  it("Belnap unread is — for each count with the reason", () => {
    const h = renderToStaticMarkup(<BelnapCounts counts={null} reason="no merge committed yet" />);
    expect((h.match(/—/g) ?? []).length).toBeGreaterThanOrEqual(4);
    expect(h).not.toMatch(/>0</);
    expect(belnapSentence({ t: 6, f: 1, both: 0, neither: 1 })).toBe("Belnap result: true 6, false 1, both 0, neither 1");
  });
  it("escrow says SALT parts exactly, and hides the bar when unread", () => {
    const e = { budgetWei: 600n * SALT, paidWei: 0n, reservedWei: 9n * SALT, refundableWei: 591n * SALT };
    expect(escrowSentence(e)).toBe("Escrow of 600.00 SALT: 0.00 paid, 9.00 reserved, 591.00 refundable");
    const h = renderToStaticMarkup(<EscrowBar escrow={e} />);
    expect(h).toContain(`role="img"`);
    expect(h).not.toMatch(/USD|\$/);
    const u = renderToStaticMarkup(<EscrowBar escrow={null} reason="WorkMarket not on this network" />);
    expect(u).not.toContain(`role="img"`);
    expect(u).toContain("WorkMarket not on this network");
  });
  it("the timeline lists five steps and names a failed step", () => {
    const h = renderToStaticMarkup(<ContributionTimeline steps={[{ key: "dataset", state: "done", detail: "support-tickets v3" }, { key: "training", state: "failed", detail: "the worker stopped" }]} />);
    expect((h.match(/data-step=/g) ?? []).length).toBe(5);
    expect(h).toContain(`data-step="training" data-state="failed"`);
    expect(h).toContain("the worker stopped");
  });
});

describe("Payout receipt", () => {
  const line = { round: "14", units: 7n, rateWei: (3n * SALT) / 2n, amountWei: (21n * SALT) / 2n, txHash: "0x" + "8f".repeat(32), block: 1204512 };
  it("says units times rate equals amount, SALT only", () => {
    expect(payoutSentence("claimable", line)).toBe("Round 14: 7 units times 1.50 SALT equals 10.50 SALT, claimable");
    const h = renderToStaticMarkup(<PayoutReceipt status="claimable" line={line} />);
    expect(h).toContain("7 units × 1.50 SALT = 10.50 SALT");
  });
  it("dark and not deployed show the exact words and no amount", () => {
    for (const st of ["dark", "not-deployed", "nothing"] as PayoutStatus[]) {
      const h = renderToStaticMarkup(<PayoutReceipt status={st} line={line} />);
      expect(h).not.toContain("10.50");
    }
    expect(renderToStaticMarkup(<PayoutReceipt status="dark" />)).toContain("Payouts open after the security review");
    expect(renderToStaticMarkup(<PayoutReceipt status="not-deployed" />)).toContain("Not available yet on this network");
    expect(renderToStaticMarkup(<PayoutReceipt status="disputed" line={line} />)).toContain("Disputed, frozen");
  });
});

describe("Ceremony outcome words (UI truth contract §1)", () => {
  it("included, reverted with fee, not sent, unknown", () => {
    expect(outcomeWords({ kind: "included", block: 1204512 }).word).toBe("Included in block 1,204,512");
    const rev = outcomeWords(outcomeFromReceipt({ status: 0, blockNumber: 1204512, gasUsed: 21000n, effectiveGasPrice: 100000000000000n, reason: "the round reached max workers" }));
    expect(rev.word).toBe("Included in block 1,204,512 · reverted");
    expect(rev.line).toContain("the fee was charged (2.1000 SALT)");
    expect(rev.line).toContain("the round reached max workers");
    expect(outcomeWords(outcomeFromReceipt({ status: 0, blockNumber: 5 })).line).toContain("fee not read");
    // #685 review: a missing or unexpected status is unknown, never "reverted, fee charged".
    for (const status of [null, undefined, 2]) {
      const o = outcomeFromReceipt({ status, blockNumber: 5, gasUsed: 21000n, effectiveGasPrice: 1n, txHash: "0x" + "ab".repeat(32) });
      expect(o.kind).toBe("unknown");
      const w = outcomeWords(o);
      expect(w.word).toMatch(/^Sent, outcome unknown/);
      expect(w.word + w.line).not.toMatch(/reverted|fee was charged/);
      expect(w.line).toContain("no usable status");
    }
    const ns = outcomeWords(outcomeFromBroadcastError("not submitted: insufficient funds"));
    expect(ns.word).toBe("Not sent");
    expect(ns.line).toContain("No fee was charged");
    const hash = "0x" + "8f".repeat(32);
    const un = outcomeWords(outcomeFromBroadcastError(`ceremony: submitted, outcome unknown (tx ${hash}): timeout`));
    expect(un.word).toBe(`Sent, outcome unknown: check tx ${shortHex(hash)}`);
    expect(un.line).toContain("Nothing is re-sent automatically");
    expect(un.line).toContain("timeout");
    expect(outcomeWords(outcomeFromBroadcastError(new Error("ipc deadline"))).word).toBe("Sent, outcome unknown");
  });
  it("never says final or settled", () => {
    for (const o of [{ kind: "included", block: 1 }, { kind: "not-sent", reason: "x" }, { kind: "unknown", txHash: null }] as const) {
      const w = outcomeWords(o);
      expect(w.word + w.line).not.toMatch(/final|settled/i);
    }
  });
});

describe("Ceremony sheet", () => {
  const mount = (step: "review" | "signing" | "done", onDecline = vi.fn()) => {
    const el = document.createElement("div");
    document.body.appendChild(el);
    const root = createRoot(el);
    act(() =>
      root.render(
        <CeremonySheet id="t" title="Join round 15" hic="HIC-1" step={step} signed={2} review={<p>This is exactly what you sign.</p>} outcome={{ kind: "included", block: 9 }} signLabel="Sign and join" onSign={() => {}} onDecline={onDecline} />,
      ),
    );
    return { el, root, onDecline };
  };
  it("starts on Decline, and Escape declines before signing", () => {
    const { el, root, onDecline } = mount("review");
    expect(document.activeElement?.textContent).toBe("Decline");
    act(() => {
      el.querySelector('[role="dialog"]')!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(onDecline).toHaveBeenCalledTimes(1);
    act(() => root.unmount());
    el.remove();
  });
  it("while signing: a status region with three named steps, and Escape does nothing", () => {
    const { el, root, onDecline } = mount("signing");
    const st = el.querySelector('[role="status"]')!;
    expect(st.getAttribute("aria-live")).toBe("polite");
    expect(st.textContent).toContain("Signed on this device");
    expect(el.querySelectorAll('[data-done="true"]').length).toBe(2);
    act(() => {
      el.querySelector('[role="dialog"]')!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(onDecline).not.toHaveBeenCalled();
    expect(el.textContent).toContain("HIC-1");
    act(() => root.unmount());
    el.remove();
  });
  it("done shows the outcome in contract words", () => {
    const { el, root } = mount("done");
    expect(el.textContent).toContain("Included in block 9");
    act(() => root.unmount());
    el.remove();
  });
});

describe("primitives and format", () => {
  it("unknown renders — with the reason, never 0", () => {
    const h = renderToStaticMarkup(<Unknown reason="gas read failed" />);
    expect(h).toContain("—");
    expect(h).toContain("gas read failed");
    expect(h).not.toMatch(/>0</);
  });
  it("chips carry their word and tone; HIC badge says what it means", () => {
    expect(renderToStaticMarkup(<Chip tone="ok">Eligible</Chip>)).toContain(`class="fl-chip fl-chip-ok"`);
    const h = renderToStaticMarkup(<HicBadge level="HIC-2" />);
    expect(h).toContain("HIC-2");
    expect(h).toContain("inside an envelope you signed");
    expect(h).not.toMatch(/HITL/);
    expect(renderToStaticMarkup(<KV rows={[{ k: "Rate", v: "1.50 SALT", mono: true }]} />)).toContain("<dt>Rate</dt>");
  });
  it("SALT from wei is exact and truncated; durations say about", () => {
    expect(saltFromWei(10_500_000_000_000_000_000n)).toBe("10.50");
    expect(saltFromWei(1_999_999_999_999_999_999n)).toBe("1.99");
    expect(saltText(4210n * SALT)).toBe("4,210.00 SALT");
    expect(saltFromWei(2_100_000_000_000_000n, 4)).toBe("0.0021");
    expect(aboutDuration(11400)).toBe("about 3 h 10 min");
    expect(aboutDuration(30)).toBe("under a minute");
  });
});

describe("authority (HIC-1 by default)", () => {
  it("is HIC-1 with no envelope, HIC-2 while one is on, HIC-1 again once it is off", () => {
    expect(selectAuthority()).toEqual(HIC1);
    const fn = vi.fn();
    const off = subscribeAuthority(fn);
    setEnvelopeForAuthority({ on: true, name: "Weeknight rounds", caps: [{ label: "joins this week", left: "1 of 3" }] });
    expect(selectAuthority()).toEqual({ level: "HIC-2", name: "Weeknight rounds", caps: [{ label: "joins this week", left: "1 of 3" }] });
    setEnvelopeForAuthority({ on: false, name: "Weeknight rounds", caps: [] });
    expect(selectAuthority()).toEqual(HIC1);
    setEnvelopeForAuthority(null);
    expect(fn).toHaveBeenCalledTimes(3);
    off();
  });
});

describe("worker terms (no core storage in this build)", () => {
  const terms: WorkerTerms = { floorSaltPerUnit: "1.20", hours: { start: "23:00", end: "07:00" }, weekdaysOnly: true, mainsOnly: true, dataClasses: ["support-tickets"] };
  it("saving is refused with a plain sentence; loading reads nothing", async () => {
    await expect(saveWorkerTerms(terms)).rejects.toThrow(TERMS_SAVE_UNAVAILABLE);
    await expect(loadWorkerTerms()).resolves.toBeNull();
  });
  it("malformed terms are refused with the problem", async () => {
    expect(termsProblem(terms)).toBeNull();
    expect(termsProblem({ ...terms, floorSaltPerUnit: "-1" })).toMatch(/floor/);
    expect(termsProblem({ ...terms, hours: { start: "25:00", end: "07:00" } })).toMatch(/Hours/);
    await expect(saveWorkerTerms({ ...terms, floorSaltPerUnit: "abc" })).rejects.toThrow(/floor/);
  });
});

describe("Groups › Rounds sub-tabs", () => {
  const SUBS: [string, string, "instrument" | "charter"][] = [
    ["groups/rounds", "Matches", "instrument"],
    ["groups/rounds/live", "Live round", "instrument"],
    ["groups/rounds/open", "Open a round", "charter"],
    ["groups/rounds/datasets", "Datasets", "charter"],
    ["groups/rounds/adapters", "Adapters", "charter"],
    ["groups/rounds/payouts", "Payouts", "charter"],
  ];
  for (const [route, label, reg] of SUBS) {
    it(`${route} is the ${label} sub-tab on ${reg}`, () => {
      expect(CANONICAL_ROUTES).toContain(route);
      const loc = locate(route);
      expect(loc).toMatchObject({ place: "groups", tab: "groups/rounds", sub: route });
      expect(registerFor(route)).toBe(reg);
    });
  }
  it("the legacy train route and groups/rounds/matches open Matches", () => {
    expect(resolveRoute("train")?.route).toBe("groups/rounds");
    expect(resolveRoute("groups/rounds/matches")?.route).toBe("groups/rounds");
  });
  it("navigation keeps five places plus Settings", async () => {
    const { PLACES } = await import("../shell/places");
    expect(PLACES.map((p) => p.id)).toEqual(["hermes", "groups", "wallet", "node", "store", "settings"]);
  });
});

describe("Groups › Rounds sub-tabs render the honest Today state", async () => {
  const { PlaceView } = await import("../shell/PlaceView");
  const { store } = await import("../shell/store");
  const { freshState } = await import("../shell/state");
  const html = (route: string) => renderToStaticMarkup(<PlaceView store={store} s={{ ...freshState("p1"), stage: "done", route }} />);
  for (const route of ["groups/rounds/live", "groups/rounds/open", "groups/rounds/datasets"]) {
    it(`${route}: Group rounds not live yet, the contract table, no banned words`, () => {
      const h = html(route);
      expect(h).toContain("Group rounds not live yet");
      expect(h).toContain(`data-testid="fl-contract-table"`);
      const el = document.createElement("div");
      el.innerHTML = h;
      el.querySelectorAll('[data-testid="place-tabs"],[data-testid="place-subtabs"]').forEach((n) => n.remove());
      expect(el.textContent ?? "").not.toMatch(BANNED);
    });
  }
  it("payouts says Not available yet on this network while WorkMarket is absent", () => {
    const h = html("groups/rounds/payouts");
    expect(h).toContain("Not available yet on this network");
    expect(h).toContain("WorkMarket is not deployed on this network, so there are no payouts to show.");
  });
  it("#695 review: the Payouts page passes the banned-words check (both panes)", () => {
    const el = document.createElement("div");
    el.innerHTML = html("groups/rounds/payouts");
    el.querySelectorAll('[data-testid="place-tabs"],[data-testid="place-subtabs"]').forEach((n) => n.remove());
    expect(el.textContent ?? "").not.toMatch(BANNED);
  });
  it("adapters keeps the eval gate (fl_rounds.rs)", () => {
    expect(html("groups/rounds/adapters")).toContain("Run the eval gate");
  });
  it("live round carries the FL band", () => {
    expect(html("groups/rounds/live")).toContain(`data-testid="fl-band"`);
  });
});
