// FL design board 07, Rounds › Open a round: one test per StateMatrix state (Empty: blank template;
// Not deployed: no Open button, save draft only; Pending: signing; Success: included in block N;
// Failure: only owners and admins; Unavailable: rate hint n = 0) plus the draft and its checks.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import realBook from "../../src-tauri/addresses/40204.json";
import type { Group } from "../bridge/domains";
import { readFlNetworkState, type FlNetworkState } from "./networkState";
import { OpenRoundView } from "./OpenRound";
import { BLANK_DRAFT, clearRoundDraft, draftProblems, leasesCovered, loadRoundDraft, setRoundDraft, type RoundDraft } from "./roundDraft";
import type { TxOutcome } from "./CeremonySheet";

const WM = "0x" + "ab".repeat(20);
const reads = { codeSize: async () => 1000, viewCall: async () => "0x" + "0".repeat(64) };
const today = () => readFlNetworkState(realBook, reads);
const live = () => readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, reads);
const G: Group = { id: "0x" + "ab".repeat(16), name: "Orchard Lab", owner: "0x" + "11".repeat(20), kind: "channel", members: [] };
const ready: RoundDraft = { ...BLANK_DRAFT, groupId: G.id, dataClass: "support-tickets", deadline: "2026-11-22" };

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
    const m = mount(await today());
    m.click("Continue");
    expect(m.el.textContent).toContain("No comparable rounds on this network yet (n = 0), so there is no suggested rate.");
    expect(m.el.querySelector('[data-testid="covers"]')!.textContent).toBe("Budget covers 400 leases at this rate.");
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
    expect(document.querySelector('[role="dialog"]')!.textContent).toContain("600 SALT leaves your wallet into WorkMarket");
    expect(document.activeElement?.textContent).toBe("Decline");
    const signs = [...document.querySelectorAll('[role="dialog"] button')].filter((b) => b.textContent === "Sign escrow and open");
    act(() => (signs[0] as HTMLButtonElement).click());
    expect(document.querySelector('[role="status"][aria-live="polite"]')!.textContent).toContain("Sent to the network, waiting for a receipt");
    await act(async () => release({ kind: "included", block: 1_204_690 }));
    const d = document.querySelector('[role="dialog"]')!;
    expect(d.textContent).toContain("Included in block 1,204,690");
    expect(d.textContent).toContain("The round is open. Escrow holds 600 SALT.");
    m.click("See it in Matches");
    expect(m.go).toHaveBeenCalledWith("groups/rounds");
    m.done();
  });
  it("Failure: a refused open is Not sent with the reason", async () => {
    const m = mount(await live(), { draft: ready, onOpen: async () => Promise.reject(new Error("WorkMarket.open is not in this build")) });
    for (let i = 0; i < 3; i++) m.click("Continue");
    m.click("Sign escrow and open");
    const signs = [...document.querySelectorAll('[role="dialog"] button')].filter((b) => b.textContent === "Sign escrow and open");
    await act(async () => (signs[0] as HTMLButtonElement).click());
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
    expect(draftProblems(BLANK_DRAFT)).toEqual(["Choose a group", "Name the data class members train on", "Set a deadline"]);
    expect(draftProblems({ ...ready, maxWorkers: 2 })).toContain("At least three workers (the round ledger's floor)");
    expect(draftProblems(ready)).toEqual([]);
  });
  it("a draft handed in by Hermes is kept on this device for the member to check", () => {
    setRoundDraft({ dataClass: "photos" }, "hermes");
    expect(loadRoundDraft()).toMatchObject({ dataClass: "photos", source: "hermes" });
    clearRoundDraft();
    expect(loadRoundDraft()).toBeNull();
  });
});
