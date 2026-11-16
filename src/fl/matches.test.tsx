// FL design board 05, Rounds › Matches: one test per StateMatrix state (Empty, Not deployed,
// Pending, Failure, Dark, Unavailable) plus the join sheet and the matcher. Offers here are
// TEST-ONLY fixtures shaped like core's fl_market_offers (PR #625); no production module builds one.
import { describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { decodeFunctionData, toFunctionSelector } from "viem";
import realBook from "../../src-tauri/addresses/40204.json";
import { readFlNetworkState, type FlReads, type FlNetworkState } from "./networkState";
import { HIC1, type Authority } from "./authority";
import { hiddenWords, machineFrom, matchOffer, offersCsv, rankMatches, weiFromSalt, type MarketView, type OfferView } from "./market";
import { JOIN_SIGNATURE, JOIN_WAITS_FOR_GSEC, joinBlocker, joinCalldata, joinIntent, MatchesView, PRIVACY_SENTENCE, WORK_MARKET_JOIN_ABI, type MarketRead } from "./Matches";
import { ACTIVE_PROFILE, type NetworkProfile } from "../network/profile";
import { saltExact } from "./format";
import type { WorkerTerms } from "./workerTerms";

const SALT = 10n ** 18n;
const WM = "0x" + "ab".repeat(20);
const ZERO32 = "0x" + "0".repeat(64);
const reads: FlReads = { codeSize: async () => 1000, viewCall: async () => ZERO32 };
const today = () => readFlNetworkState(realBook, reads);
const live = () => readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, reads);
/** Test-only: a profile that opens payouts (no shipped profile does). */
const opened = () => readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, reads, { ...(ACTIVE_PROFILE as NetworkProfile), payoutsOpen: true } as NetworkProfile);

const offer = (id: number, over: { price?: bigint; minVram?: number; classes?: string[]; hidden?: OfferView["hidden"]; allow?: string; full?: boolean; deadline?: number } = {}): OfferView => ({
  opened: { id, kind: "flRound", poster: "0x" + String(id).padStart(2, "0").repeat(20), groupHash: ZERO32, pricePerUnit: String(over.price ?? (3n * SALT) / 2n), budget: String(600n * SALT), deadline: over.deadline ?? 1_763_769_600 + id, requirementsCid: "bafkreiflm" + id },
  openedBlock: 100 + id,
  closed: null,
  terms: { state: "open", jobSpecHash: ZERO32, requirementsHash: ZERO32, allowlistRoot: over.allow ?? ZERO32, maxWorkers: 8, workers: over.full ? 8 : 3, joinBond: String(SALT / 10n), coordinator: "0x" + "cd".repeat(20), disputeWindow: 100 },
  requirements: { status: "verified", requirements: { v: 1, dataClasses: over.classes ?? ["support-tickets"], architectures: ["llama-8b-lora-r8"], minVramGb: over.minVram ?? 16, estHours: 2 } },
  hidden: over.hidden ?? null,
});

const terms: WorkerTerms = { floorSaltPerUnit: "1.20", hours: { start: "23:00", end: "07:00" }, weekdaysOnly: true, mainsOnly: true, dataClasses: ["support-tickets"] };
const machine = { memoryGb: 24, memoryKind: "VRAM" as const };

function render(state: FlNetworkState, market: MarketRead, extra: { authority?: Authority; onJoin?: (o: OfferView) => Promise<void>; terms?: WorkerTerms | null } = {}) {
  const el = document.createElement("div");
  document.body.appendChild(el);
  const root = createRoot(el);
  const go = vi.fn();
  act(() =>
    root.render(
      <MatchesView
        state={state}
        band={<div data-testid="band" />}
        market={market}
        terms={extra.terms === undefined ? terms : extra.terms}
        machine={machine}
        authority={extra.authority ?? HIC1}
        go={go}
        onJoin={extra.onJoin ?? (async () => {})}
        coordinator={<div data-testid="coord-panel" />}
        save={async (n) => `Saved ${n} to Downloads`}
      />,
    ),
  );
  const click = (text: string, scope: ParentNode = document.body) => {
    const b = [...scope.querySelectorAll("button")].find((x) => x.textContent?.trim() === text);
    if (!b) throw new Error("no button " + text);
    act(() => b.click());
  };
  const done = () => {
    act(() => root.unmount());
    el.remove();
  };
  return { el, go, click, done };
}

describe("matcher", () => {
  it("inside your terms says why", () => {
    const m = matchOffer(offer(15), terms, machine);
    expect(m).toMatchObject({ joinable: true, chip: "Inside your terms", whyLabel: "Why this match:" });
    expect(m.why).toContain("needs 16 GB (you have 24)");
    expect(m.why).toContain("at or above your floor");
  });
  it("names every reason it cannot be joined", () => {
    expect(matchOffer(offer(9, { price: (4n * SALT) / 5n }), terms, machine).why).toContain("below your floor (0.80 < 1.20)");
    expect(matchOffer(offer(16, { minVram: 48 }), terms, machine).why).toContain("needs 48 GB VRAM; this machine has 24 GB");
    expect(matchOffer(offer(17, { classes: ["photos"] }), terms, machine).why).toContain("reads photos data, which your terms do not allow");
    expect(matchOffer(offer(18, { full: true }), terms, machine).why).toContain("all 8 worker places are taken");
    expect(matchOffer(offer(19), terms, { memoryGb: null, memoryKind: null }).why).toContain("GPU memory was not read");
  });
  it("without terms: joinable on the offer's own rules, and says so", () => {
    const m = matchOffer(offer(15), null, machine);
    expect(m.chip).toBe("No terms set");
    expect(m.why).toContain("you have not set terms");
  });
  it("ranks joinable first, then by deadline, never by amount", () => {
    const ms = rankMatches([matchOffer(offer(1, { minVram: 48, deadline: 1 }), terms, machine), matchOffer(offer(2, { price: 9n * SALT, deadline: 9 }), terms, machine), matchOffer(offer(3, { deadline: 5 }), terms, machine)]);
    expect(ms.map((m) => m.offer.opened.id)).toEqual([3, 2, 1]);
  });
  it("machine memory comes from the hardware probe; unified memory on Apple Silicon", () => {
    expect(machineFrom({ os: "linux", arch: "x86_64", totalRamBytes: 64 * 1024 ** 3, unifiedMemory: false, gpuVramBytes: 24 * 1024 ** 3, diskFreeBytes: null })).toEqual({ memoryGb: 24, memoryKind: "VRAM" });
    expect(machineFrom({ os: "macos", arch: "aarch64", totalRamBytes: 32 * 1024 ** 3, unifiedMemory: true, gpuVramBytes: null, diskFreeBytes: null })).toEqual({ memoryGb: 32, memoryKind: "unified memory" });
    expect(machineFrom(null)).toEqual({ memoryGb: null, memoryKind: null });
    expect(weiFromSalt("1.20")).toBe((6n * SALT) / 5n);
    expect(weiFromSalt("x")).toBeNull();
  });
});

describe("join intent", () => {
  it("encodes WorkMarket.join(id, minPrice = the reviewed rate, no proof)", () => {
    const data = joinCalldata(offer(15));
    expect(data.slice(0, 10)).toBe(toFunctionSelector(JOIN_SIGNATURE));
    const d = decodeFunctionData({ abi: WORK_MARKET_JOIN_ABI, data: data as `0x${string}` });
    expect(d.args).toEqual([15n, (3n * SALT) / 2n, []]);
  });
  it("refuses to build a join for an allowlisted round or unread terms", () => {
    expect(joinBlocker(offer(15))).toBeNull();
    expect(joinBlocker(offer(15, { allow: "0x" + "11".repeat(32) }))).toMatch(/allowlist/);
    expect(joinBlocker({ ...offer(15), terms: null })).toMatch(/terms/);
  });
});

describe("Matches states (StateMatrix)", () => {
  it("Not deployed (ship default): Group rounds not live yet, contract rows, no Join anywhere", async () => {
    const r = render(await today(), { kind: "idle" });
    expect(r.el.textContent).toContain("Group rounds not live yet");
    expect(r.el.textContent).toContain("Rounds open on this network after the WorkMarket contract is deployed.");
    expect(r.el.querySelectorAll('[data-contract]').length).toBe(4);
    expect([...r.el.querySelectorAll("button")].map((b) => b.textContent)).not.toContain("Join");
    r.click("Check an adapter file");
    expect(r.go).toHaveBeenCalledWith("groups/rounds/adapters");
    r.done();
  });
  it("Not deployed: Advanced says there are no offers and holds the coordinator rounds", async () => {
    const r = render(await today(), { kind: "idle" });
    r.click("Advanced");
    const d = document.querySelector('[role="dialog"]')!;
    expect(d.textContent).toContain("WorkMarket is not on this network, so there are no offers to list");
    expect(d.querySelector('[data-testid="coord-panel"]')).not.toBeNull();
    expect(d.textContent).toContain("Recorded for the cooperative's books; payment is WorkMarket's.");
    r.click("Close", d);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    r.done();
  });
  it("Empty: No open rounds right now", async () => {
    const r = render(await live(), { kind: "read", view: { available: true, reason: null, market: WM, offers: [] } });
    expect(r.el.textContent).toContain("No open rounds right now");
    r.done();
  });
  it("Unavailable: a failed offers read is — with the reason, never an empty list", async () => {
    const r = render(await live(), { kind: "failed", reason: "coordinator not answering" });
    expect(r.el.querySelector('[data-testid="matches-unread"]')?.textContent).toContain("coordinator not answering");
    expect(r.el.textContent).not.toContain("No open rounds right now");
    r.done();
  });
  it("Dark: the payouts card says Payouts open after the security review", async () => {
    const r = render(await live(), { kind: "read", view: { available: true, reason: null, market: WM, offers: [offer(15)] } });
    expect(r.el.textContent).toContain("Payouts open after the security review");
    r.done();
  });
  it("matches show why, and only joinable ones have Join", async () => {
    const view: MarketView = { available: true, reason: null, market: WM, offers: [offer(15), offer(16, { minVram: 48 }), offer(11, { hidden: { reason: "posterNotAdmin" } })] };
    const r = render(await live(), { kind: "read", view });
    const cards = [...r.el.querySelectorAll('[data-testid="match-card"]')];
    expect(cards.map((c) => c.getAttribute("data-offer"))).toEqual(["15", "16"]);
    expect(cards[0].textContent).toContain("Why this match:");
    expect(cards[1].textContent).toContain("Why not:");
    expect(cards[1].querySelector("button[disabled]")?.textContent).toBe("Can't join");
    r.click("Advanced");
    const d = document.querySelector('[role="dialog"]')!;
    expect(d.textContent).toContain("hidden · poster is not an owner or admin on the live roster (FLM-CORE-1)");
    r.click("Export as CSV", d);
    await act(async () => {});
    expect(d.textContent).toContain("Saved offers.csv to Downloads");
    r.done();
  });
});

describe("join sheet", () => {
  const view: MarketView = { available: true, reason: null, market: WM, offers: [offer(15)] };
  it("review: exactly what you sign, the privacy sentence, fee —, starts on Decline", async () => {
    const r = render(await opened(), { kind: "read", view });
    r.click("Join");
    const d = document.querySelector('[role="dialog"]')!;
    expect(d.textContent).toContain("This is exactly what you sign.");
    expect(d.textContent).toContain("WorkMarket offer 15");
    expect(d.textContent).toContain(PRIVACY_SENTENCE);
    expect(d.textContent).toContain("the wallet review reads gas before you sign");
    expect(d.textContent).toContain("No training starts from the app");
    expect(d.textContent).toContain("HIC-1");
    expect(document.activeElement?.textContent).toBe("Decline");
    r.click("Decline", d);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    r.done();
  });
  it("Pending: signing hands the join to the wallet review, then the sheet steps aside", async () => {
    let release!: () => void;
    const onJoin = vi.fn(() => new Promise<void>((res) => (release = res)));
    const r = render(await opened(), { kind: "read", view }, { onJoin });
    r.click("Join");
    r.click("Sign and join");
    expect(onJoin).toHaveBeenCalledWith(view.offers[0]);
    expect(document.querySelector('[role="status"]')?.textContent).toContain("Reviewed by you");
    await act(async () => release());
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    r.done();
  });
  it("Failure: a refused hand-off is Not sent with the reason, back on review", async () => {
    const r = render(await opened(), { kind: "read", view }, { onJoin: async () => Promise.reject(new Error("another approval is already waiting")) });
    r.click("Join");
    r.click("Sign and join");
    await act(async () => {});
    expect(document.querySelector('[role="alert"]')?.textContent).toBe("Not sent: another approval is already waiting");
    expect(document.querySelector('[data-step="review"]')).not.toBeNull();
    r.done();
  });
  it("an envelope never skips the first join with a new poster", async () => {
    const r = render(await opened(), { kind: "read", view }, { authority: { level: "HIC-2", name: "Weeknights", caps: [] } });
    r.click("Join");
    expect(document.querySelector('[role="dialog"]')!.textContent).toContain("The first join with any new group or poster always asks you.");
    r.done();
  });
});

describe("G-SEC review fixes (PAY-10, PAY-12, PAY-18)", () => {
  const view: MarketView = { available: true, reason: null, market: WM, offers: [offer(15)] };
  it("PAY-10: with payouts dark there is no Join anywhere, and the reason is said", async () => {
    const r = render(await live(), { kind: "read", view });
    expect([...r.el.querySelectorAll("button")].map((b) => b.textContent)).not.toContain("Join");
    expect(r.el.textContent).toContain("joining posts a SALT join bond, so it opens with payouts after the security review");
    r.done();
  });
  it("PAY-10: the join intent refuses to exist while payouts are dark (r1007 and any dark profile)", async () => {
    const dark = await live();
    const r1007 = await today();
    expect(() => joinIntent(offer(15), dark)).toThrow(JOIN_WAITS_FOR_GSEC);
    expect(() => joinIntent(offer(15), r1007)).toThrow(JOIN_WAITS_FOR_GSEC);
  });
  it("PAY-18: an unread join bond fails closed, never value 0", async () => {
    const st = await opened();
    expect(() => joinIntent({ ...offer(15), terms: null }, st)).toThrow(/terms/);
    const bad = { ...offer(15), terms: { ...offer(15).terms!, joinBond: "" } };
    expect(() => joinIntent(bad, st)).toThrow("the offer's join bond was not read, so nothing was sent");
    expect(joinIntent(offer(15), st)).toMatchObject({ address: WM, valueWei: String(SALT / 10n), label: JOIN_SIGNATURE });
  });
  it("PAY-12: the review shows every decimal of what is signed, and the wei", async () => {
    const tiny = { ...offer(15, { price: 90_000_000_000_000n }), terms: { ...offer(15).terms!, joinBond: "123456789012345678" } };
    const r = render(await opened(), { kind: "read", view: { ...view, offers: [tiny] } }, { terms: null });
    r.click("Join");
    const d = document.querySelector('[role="dialog"]')!;
    expect(d.textContent).toContain("0.00009 SALT per unit (90000000000000 wei)");
    expect(d.textContent).toContain("0.123456789012345678 SALT (123456789012345678 wei), returned when you settle");
    expect(d.textContent).not.toContain("0.0000 SALT");
    expect(saltExact(1n)).toBe("0.000000000000000001");
    expect(saltExact(10n ** 18n)).toBe("1.00");
    r.done();
  });
});

describe("offers export", () => {
  it("lists every offer with why it is hidden, SALT only", () => {
    const csv = offersCsv({ available: true, reason: null, market: WM, offers: [offer(15), offer(11, { hidden: { reason: "requirementsRefused", detail: "hash mismatch" } })] });
    expect(csv.split("\n")[0]).toBe("offer,poster,price_per_unit_salt,deadline,requirements_cid,shown,why_hidden");
    expect(csv).toContain("1.5000");
    expect(csv).toContain(hiddenWords({ reason: "requirementsRefused", detail: "hash mismatch" }));
    expect(csv).not.toMatch(/USD|\$/);
  });
  it("static render of the Today state has no banned copy", async () => {
    const h = renderToStaticMarkup(
      <MatchesView state={await today()} band={null} market={{ kind: "idle" }} terms={null} machine={machine} authority={HIC1} go={() => {}} onJoin={async () => {}} coordinator={null} save={async () => ""} />,
    );
    const el = document.createElement("div");
    el.innerHTML = h;
    expect(el.textContent).not.toMatch(/\b(earn\w*|income|projected|estimate|yield|APY|APR|profit|USD|dormant|Final|Settled|HITL|users?)\b|!/);
  });
});
