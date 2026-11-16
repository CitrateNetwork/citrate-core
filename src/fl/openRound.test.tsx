// FL design board 07, Rounds › Open a round: one test per StateMatrix state (Empty: blank template;
// Not deployed: no Open button, save draft only; Pending: signing; Success: included in block N;
// Failure: only owners and admins; Unavailable: rate hint n = 0) plus the draft and its checks.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import realBook from "../../src-tauri/addresses/40204.json";
import type { Group } from "../bridge/domains";
import { readFlNetworkState, type FlNetworkState } from "./networkState";
import { ACTIVE_PROFILE, type NetworkProfile } from "../network/profile";
import { OpenRoundView } from "./OpenRound";
import { BLANK_DRAFT, clearRoundDraft, draftProblems, leasesCovered, loadRoundDraft, openParamsOf, setRoundDraft, type RoundDraft } from "./roundDraft";
import type { TxOutcome } from "./CeremonySheet";

const WM = "0x" + "ab".repeat(20);
const reads = { codeSize: async () => 1000, viewCall: async () => "0x" + "0".repeat(64) };
const today = () => readFlNetworkState(realBook, reads);
const dark = () => readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, reads);
/** Test-only: a profile that opens payouts (no shipped profile does). */
const live = () => readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, reads, { ...(ACTIVE_PROFILE as NetworkProfile), payoutsOpen: true } as NetworkProfile);
const G: Group = { id: "0x" + "ab".repeat(16), name: "Orchard Lab", owner: "0x" + "11".repeat(20), kind: "channel", members: [] };
const ready: RoundDraft = { ...BLANK_DRAFT, groupId: G.id, dataClass: "support-tickets", deadline: "2026-11-22", budgetSalt: "600", rateSalt: "1.50", joinBondSalt: "0.1" };

function mount(state: FlNetworkState, o: { draft?: RoundDraft; role?: string | null; onOpen?: (d: RoundDraft) => Promise<TxOutcome>; save?: (d: RoundDraft) => number } = {}) {
  const el = document.createElement("div");
  document.body.appendChild(el);
  const root = createRoot(el);
  const go = vi.fn();
  act(() =>
    root.render(
      <OpenRoundView state={state} draft={o.draft ?? BLANK_DRAFT} groups={[G]} roleOf={() => (o.role === undefined ? "admin" : o.role)} coordinatorUrl={null} onOpen={o.onOpen ?? (async () => ({ kind: "included", block: 1 }))} save={o.save ?? (() => Date.UTC(2026, 10, 15, 21, 20))} go={go} />,
    ),
  );
  const click = (t: string) => {
    const b = [...document.querySelectorAll("button")].find((x) => x.textContent?.trim().endsWith(t));
    if (!b) throw new Error("no button " + t);
    act(() => b.click());
  };
  const buttons = () => [...document.querySelectorAll("button")].map((b) => b.textContent?.trim());
  return { el, go, click, buttons, done: () => (act(() => root.unmount()), el.remove()) };
}

afterEach(() => clearRoundDraft());

describe("Open a round states (StateMatrix)", () => {
  it("Empty: a blank template on step 1", async () => {
    const m = mount(await live());
    expect(m.el.textContent).toContain("What should the round do?");
    expect(m.el.querySelectorAll('[role="radio"]').length).toBe(3);
    m.done();
  });
  it("Not deployed (ship default): no Open button, Save draft only", async () => {
    const m = mount(await today(), { draft: ready });
    expect(m.el.querySelector('[data-testid="open-not-live"]')!.textContent).toBe("Group rounds not live yet. WorkMarket is not available yet on this network, so there is no Open button. You can save this draft.");
    for (let i = 0; i < 3; i++) m.click("Continue");
    expect(m.buttons().some((b) => b?.includes("Sign escrow and open"))).toBe(false);
    m.click("Save draft");
    expect(m.el.textContent).toMatch(/Draft saved on this device at \d\d:\d\d/);
    m.done();
  });
  it("Unavailable: the rate hint says no comparable rounds (n = 0), never a made-up rate", async () => {
    const m = mount(await today(), { draft: ready });
    m.click("Continue");
    expect(m.el.textContent).toContain("No comparable rounds on this network yet (n = 0), so there is no suggested rate.");
    expect(m.el.querySelector('[data-testid="covers"]')!.textContent).toBe("Budget covers 400 leases at this rate.");
    m.done();
  });
  it("PAY-19: money is never prefilled; a blank draft covers nothing", async () => {
    expect([BLANK_DRAFT.budgetSalt, BLANK_DRAFT.rateSalt, BLANK_DRAFT.joinBondSalt, BLANK_DRAFT.coordinatorBondSalt]).toEqual(["", "", "", ""]);
    const m = mount(await today());
    m.click("Continue");
    const inputs = [...m.el.querySelectorAll<HTMLInputElement>('input[inputmode="decimal"]')];
    expect(inputs.length).toBeGreaterThanOrEqual(4);
    expect(inputs.every((i) => i.value === "")).toBe(true);
    expect(m.el.querySelector('[data-testid="covers"]')!.textContent).toContain("budget and rate must be SALT amounts");
    m.done();
  });
  it("PAY-19: the preview shows the parsed wei of every amount, not the typed string", async () => {
    const m = mount(await today(), { draft: { ...ready, budgetSalt: "600.000000000000000001", rateSalt: "1.5" } });
    for (let i = 0; i < 3; i++) m.click("Continue");
    expect(m.el.textContent).toContain("600.000000000000000001 SALT (600000000000000000001 wei)");
    expect(m.el.textContent).toContain("1.50 SALT (1500000000000000000 wei) per lease");
    expect(m.el.textContent).toContain("0.10 SALT (100000000000000000 wei)");
    expect(m.el.textContent).toContain("WorkMarket's default");
    m.done();
  });
  it("Failure: only owners and admins can open a round", async () => {
    const m = mount(await live(), { draft: ready, role: "member" });
    expect(m.el.querySelector('[role="alert"]')!.textContent).toBe("Only owners and admins can open a round. You can read and comment on the draft.");
    for (let i = 0; i < 3; i++) m.click("Continue");
    expect(m.buttons().some((b) => b?.includes("Sign escrow and open"))).toBe(false);
    m.done();
  });
  it("Pending then Success: signing, then Included in block N with the escrow line", async () => {
    let release!: (o: TxOutcome) => void;
    const onOpen = vi.fn(() => new Promise<TxOutcome>((r) => (release = r)));
    const m = mount(await live(), { draft: ready, onOpen });
    for (let i = 0; i < 3; i++) m.click("Continue");
    m.click("Sign escrow and open");
    expect(document.querySelector('[role="dialog"]')!.textContent).toContain("600.00 SALT (600000000000000000000 wei) leaves your wallet into WorkMarket");
    expect(document.activeElement?.textContent).toBe("Decline");
    const signs = [...document.querySelectorAll('[role="dialog"] button')].filter((b) => b.textContent === "Sign escrow and open");
    act(() => (signs[0] as HTMLButtonElement).click());
    expect(document.querySelector('[role="status"][aria-live="polite"]')!.textContent).toContain("Sent to the network, waiting for a receipt");
    await act(async () => release({ kind: "included", block: 1_204_690 }));
    const d = document.querySelector('[role="dialog"]')!;
    expect(d.textContent).toContain("Included in block 1,204,690");
    expect(d.textContent).toContain("The round is open. Escrow holds 600.00 SALT (600000000000000000000 wei).");
    m.click("See it in Matches");
    expect(m.go).toHaveBeenCalledWith("groups/rounds");
    m.done();
  });
  it("PAY-10: with payouts dark there is no Sign, only the reason and Save draft", async () => {
    const m = mount(await dark(), { draft: ready });
    for (let i = 0; i < 3; i++) m.click("Continue");
    expect(m.buttons().some((b) => b?.includes("Sign escrow and open"))).toBe(false);
    expect(m.el.querySelector('[data-testid="open-waits-gsec"]')!.textContent).toContain("opens with payouts after the security review");
    m.done();
  });
  it("Failure: a refused open is Not sent with the reason", async () => {
    const m = mount(await live(), { draft: ready, onOpen: async () => Promise.reject(new Error("WorkMarket.open is not in this build")) });
    for (let i = 0; i < 3; i++) m.click("Continue");
    m.click("Sign escrow and open");
    const signs = [...document.querySelectorAll('[role="dialog"] button')].filter((b) => b.textContent === "Sign escrow and open");
    await act(async () => (signs[0] as HTMLButtonElement).click());
    await act(async () => {});
    expect(document.querySelector('[role="dialog"]')!.textContent).toContain("Not sent");
    expect(document.querySelector('[role="dialog"]')!.textContent).toContain("WorkMarket.open is not in this build");
    m.done();
  });
  it("a Hermes draft says so, and Hermes cannot sign", async () => {
    const m = mount(await live(), { draft: { ...ready, source: "hermes", draftedFor: "Rui in the Orchard Lab chat" } });
    expect(m.el.textContent).toContain("Drafted by Hermes");
    expect(m.el.textContent).toContain("For Rui in the Orchard Lab chat. Check every field; Hermes cannot sign.");
    m.done();
  });
});

describe("round drafts", () => {
  it("budget covers floor(budget / rate) leases, exactly", () => {
    expect(leasesCovered("600", "1.50")).toBe(400);
    expect(leasesCovered("10", "3")).toBe(3);
    expect(leasesCovered("x", "1")).toBeNull();
    expect(leasesCovered("1", "0")).toBeNull();
  });
  it("names what a draft still needs", () => {
    expect(draftProblems(BLANK_DRAFT)).toEqual(["Choose a group", "Name the data class members train on", "Enter the budget and the rate as SALT amounts", "Set a deadline", "Enter the join bond as a SALT amount (0 for none)"]);
    expect(draftProblems({ ...ready, disputeWindowHours: 23 })).toContain("The dispute window must be at least 24 hours (WorkMarket's floor)");
    expect(draftProblems({ ...ready, maxWorkers: 2 })).toContain("At least three workers (the round ledger's floor)");
    expect(draftProblems(ready)).toEqual([]);
  });
  it("PAY-19: openParamsOf builds WorkMarket.OpenParams field for field, exact in wei", () => {
    const b = { verifier: "0x" + "22".repeat(20), bindRef: "0x" + "3e".repeat(32), jobSpecHash: "0x" + "44".repeat(32), requirementsHash: "0x" + "55".repeat(32), requirementsCid: "bafkreiflm1", groupHash: "0x" + "66".repeat(32), coordinator: "0x" + "cd".repeat(20) };
    const r = openParamsOf({ ...ready, coordinatorBondSalt: "100", disputeWindowHours: 48 }, b);
    if (!("params" in r)) throw new Error("expected params");
    expect(Object.keys(r.params)).toEqual(["kind", "verifier", "bindRef", "jobSpecHash", "requirementsHash", "requirementsCid", "groupHash", "allowlistRoot", "pricePerUnit", "deadline", "disputeWindow", "maxWorkers", "joinBond", "coordinator", "coordinatorBond", "disputeAdmin"]);
    expect(r.valueWei).toBe(600n * 10n ** 18n);
    expect(r.params).toMatchObject({ kind: 0, pricePerUnit: 15n * 10n ** 17n, joinBond: 10n ** 17n, coordinatorBond: 100n * 10n ** 18n, disputeWindow: 172_800, maxWorkers: 8, deadline: BigInt(Date.UTC(2026, 10, 22, 23, 59, 59) / 1000), allowlistRoot: "0x" + "0".repeat(64) });
    expect((openParamsOf(ready, b) as { params: { coordinatorBond: bigint } }).params.coordinatorBond).toBe(0n);
    expect(openParamsOf(BLANK_DRAFT, b)).toHaveProperty("problems");
  });
  it("PAY-19 (owner): a Hermes draft never sets money; budget, rate and bonds arrive empty", async () => {
    setRoundDraft({ dataClass: "support-tickets", budgetSalt: "600", rateSalt: "1.50", joinBondSalt: "0.1", coordinatorBondSalt: "100" }, "hermes");
    const d = loadRoundDraft()!;
    expect(d).toMatchObject({ dataClass: "support-tickets", source: "hermes", budgetSalt: "", rateSalt: "", joinBondSalt: "", coordinatorBondSalt: "" });
    const m = mount(await live(), { draft: d });
    m.click("Continue");
    const inputs = [...m.el.querySelectorAll<HTMLInputElement>('input[inputmode="decimal"]')];
    expect(inputs.every((i) => i.value === "")).toBe(true);
    m.done();
    // A member's own draft keeps what the member typed.
    setRoundDraft({ budgetSalt: "600" }, "member");
    expect(loadRoundDraft()!.budgetSalt).toBe("600");
  });
  it("PAY-19 follow-up: a Hermes hand-in the member fills in and saves becomes the member's; opening alone does not", async () => {
    setRoundDraft({ dataClass: "support-tickets", budgetSalt: "600" }, "hermes");
    const handed = loadRoundDraft()!;
    let stored: RoundDraft | null = null;
    const save = (d: RoundDraft) => {
      stored = d;
      return 1;
    };
    // Opening and saving without typing any money keeps it Hermes's.
    const a = mount(await live(), { draft: handed, save });
    a.click("Save draft");
    expect(stored!.source).toBe("hermes");
    a.done();
    // The member types the money on the board and saves: it is now the member's, money kept.
    const m = mount(await live(), { draft: handed, save });
    m.click("Continue");
    const [budget, rate] = [...m.el.querySelectorAll<HTMLInputElement>('input[inputmode="decimal"]')];
    const type = (i: HTMLInputElement, v: string) =>
      act(() => {
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(i, v);
        i.dispatchEvent(new Event("input", { bubbles: true }));
      });
    type(budget, "600");
    type(rate, "1.50");
    m.click("Save draft");
    expect(stored!).toMatchObject({ source: "member", budgetSalt: "600", rateSalt: "1.50", draftedBy: "hermes" });
    expect(m.el.textContent).toContain("Drafted by Hermes");
    m.done();
  });
  it("a draft handed in by Hermes is kept on this device for the member to check", () => {
    setRoundDraft({ dataClass: "photos" }, "hermes");
    expect(loadRoundDraft()).toMatchObject({ dataClass: "photos", source: "hermes" });
    clearRoundDraft();
    expect(loadRoundDraft()).toBeNull();
  });
});

describe("openIntent (WorkMarket.open, HC's fl_round_open_propose calls it)", async () => {
  const { openIntent, OPEN_NEEDS_BINDING, OPEN_SIGNATURE, WORK_MARKET_OPEN_ABI } = await import("./roundDraft");
  const { decodeFunctionData, toFunctionSelector } = await import("viem");
  const filled: RoundDraft = { ...BLANK_DRAFT, groupId: "0x" + "ab".repeat(16), dataClass: "support-tickets", deadline: "2026-11-22", budgetSalt: "600", rateSalt: "1.50", joinBondSalt: "0.1" };
  const b = { verifier: "0x" + "22".repeat(20), bindRef: "0x" + "3e".repeat(32), jobSpecHash: "0x" + "44".repeat(32), requirementsHash: "0x" + "55".repeat(32), requirementsCid: "bafkreiflm1", groupHash: "0x" + "66".repeat(32), coordinator: "0x" + "cd".repeat(20) };
  it("refuses while payouts are dark, with core's reason", async () => {
    const st = await readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, { ...reads, payoutsStatus: async () => ({ open: false, reason: "payouts are not open on r1007" }) });
    expect(() => openIntent(filled, b, st)).toThrow("payouts are not open on r1007");
  });
  it("refuses with no round binding, then on a bad draft", async () => {
    const st = await live();
    expect(() => openIntent(filled, null, st)).toThrow(OPEN_NEEDS_BINDING);
    expect(() => openIntent(BLANK_DRAFT, b, st)).toThrow(/Choose a group/);
  });
  it("encodes WorkMarket.open(OpenParams) with msg.value = the budget", async () => {
    const st = await live();
    const i = openIntent(filled, b, st);
    expect(i).toMatchObject({ address: WM, valueWei: String(600n * 10n ** 18n), label: OPEN_SIGNATURE });
    expect(i.calldata.slice(0, 10)).toBe(toFunctionSelector(OPEN_SIGNATURE));
    const d = decodeFunctionData({ abi: WORK_MARKET_OPEN_ABI, data: i.calldata as `0x${string}` });
    expect(d.args[0]).toMatchObject({ kind: 0, pricePerUnit: 15n * 10n ** 17n, joinBond: 10n ** 17n, requirementsCid: "bafkreiflm1", maxWorkers: 8, disputeWindow: 86_400 });
  });
});
