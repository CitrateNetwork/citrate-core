// FL design board 10, Rounds › Payouts: one test per StateMatrix state (Empty: nothing to claim;
// Not deployed; Pending: waiting for dispute window; Success: claimed; Failure: disputed, frozen;
// Dark; Unavailable: amount — with reason) plus the claim ceremony and history. Settled lines are
// TEST-ONLY fixtures; this build has no settled-lines read model.
import { describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import realBook from "../../src-tauri/addresses/40204.json";
import { ACTIVE_PROFILE, type NetworkProfile } from "../network/profile";
import type { FlOverview } from "../bridge/domains";
import { readFlNetworkState, type FlNetworkState } from "./networkState";
import { filterRows, historyCsv, historyRows, PayoutsView, type Receipt } from "./Payouts";

const SALT = 10n ** 18n;
const WM = "0x" + "ab".repeat(20);
const reads = { codeSize: async () => 1000, viewCall: async () => "0x" + "0".repeat(64) };
const nd = () => readFlNetworkState(realBook, reads);
const dark = () => readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, reads);
const open = () => readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, reads, { ...(ACTIVE_PROFILE as NetworkProfile), payoutsOpen: true } as NetworkProfile);

const ov: FlOverview = {
  config: { url: null, source: "none", settingsUrl: null, note: null },
  starts: [
    { planHash: "0x" + "9a".repeat(32), coordinatorUrl: "https://c.example", requires: "federated", authorizedAtMs: Date.UTC(2026, 10, 14, 21, 10), roundId: "0x" + "3e".repeat(32), revokedAtMs: Date.UTC(2026, 10, 15, 9, 0) },
  ],
  gates: [
    { adapterSha256: "a77d".padEnd(64, "0"), adapterPath: "/a.gguf", baseModel: "b", decidedAtMs: Date.UTC(2026, 10, 15, 21, 38), round: null, decision: { verdict: "ACCEPT", reasons: [], metrics: [], compositeBase: 0.7, compositeCandidate: 0.719 } },
  ],
  activeAdapter: null,
  rememberedAdapter: null,
  restoreError: null,
  consentedRounds: [],
  consentFile: null,
  consentError: null,
  storeError: null,
};

const line = { round: "14", units: 7n, rateWei: (3n * SALT) / 2n, amountWei: (21n * SALT) / 2n, txHash: "0x" + "4e".repeat(32), block: 1_215_002 };

function mount(state: FlNetworkState, extra: { receipts?: Receipt[] | null; pullWei?: bigint | null; act?: () => Promise<void>; ov?: FlOverview | null } = {}) {
  const el = document.createElement("div");
  document.body.appendChild(el);
  const root = createRoot(el);
  const actFn = vi.fn(extra.act ?? (async () => {}));
  act(() =>
    root.render(
      <PayoutsView
        state={state}
        receipts={extra.receipts === undefined ? null : extra.receipts}
        pullWei={extra.pullWei === undefined ? null : extra.pullWei}
        wallet={"0x5b02" + "0".repeat(32) + "19fe"}
        chainNow={Date.UTC(2026, 10, 20, 10, 0) / 1000}
        ov={extra.ov === undefined ? ov : extra.ov}
        ovError={null}
        go={() => {}}
        save={async (n) => `Saved ${n} to Downloads`}
        act={actFn}
      />,
    ),
  );
  const click = (t: string) => {
    const b = [...document.querySelectorAll("button")].find((x) => x.textContent?.trim() === t);
    if (!b) throw new Error("no button " + t);
    act(() => b.click());
  };
  return { el, click, actFn, done: () => (act(() => root.unmount()), el.remove()) };
}

describe("Payouts states (StateMatrix)", () => {
  it("Not deployed (ship default): the exact words, no amount, nothing to sign", async () => {
    const m = mount(await nd());
    expect(m.el.textContent).toContain("Not available yet on this network");
    expect(m.el.textContent).toContain("WorkMarket is not deployed on this network, so there are no payouts to show.");
    expect(m.el.textContent).toContain("WorkMarket is not on this network");
    expect([...m.el.querySelectorAll("button")].map((b) => b.textContent)).not.toContain("Claim");
    expect(m.el.querySelector<HTMLButtonElement>("button:not([role])[disabled]")?.textContent).toBe("Withdraw to wallet");
    m.done();
  });
  it("Dark: Payouts open after the security review; settled lines show but cannot be claimed", async () => {
    const m = mount(await dark(), { receipts: [{ line, status: "claimable" }], pullWei: 0n });
    expect(m.el.querySelector('[data-testid="payouts-dark"]')!.textContent).toContain("Payouts open after the security review");
    expect(m.el.querySelector('[data-testid="payout-receipt"]')!.getAttribute("data-status")).toBe("dark");
    expect(m.el.textContent).not.toContain("10.50");
    expect([...m.el.querySelectorAll("button")].map((b) => b.textContent)).not.toContain("Claim");
    m.done();
  });
  it("Dark: core's payouts gate refusal is shown", async () => {
    const st = await readFlNetworkState({ addresses: { ...realBook.addresses, WorkMarket: WM } }, { ...reads, payoutsStatus: async () => ({ open: false, reason: "WorkMarket's code does not match its pin" }) });
    const m = mount(st);
    expect(m.el.querySelector('[data-testid="payouts-reason"]')!.textContent).toBe("WorkMarket's code does not match its pin");
    m.done();
  });
  it("Unavailable: receipts and the pull balance not read are — with the reason, never 0", async () => {
    const m = mount(await dark());
    expect(m.el.textContent).toContain("this build has no settled-lines read model yet");
    expect(m.el.textContent).toContain("the WorkMarket pull balance was not read");
    expect(m.el.textContent).not.toContain("0.00 SALT");
    m.done();
  });
  it("Empty: Nothing to claim", async () => {
    const m = mount(await open(), { receipts: [], pullWei: 0n });
    expect(m.el.textContent).toContain("Nothing to claim");
    m.done();
  });
  it("Pending: Waiting for dispute window, with a Dispute action", async () => {
    const m = mount(await open(), { receipts: [{ line, status: "waiting", disputeEndsAt: Date.UTC(2026, 10, 20, 12, 0) / 1000 }], pullWei: 0n });
    expect(m.el.textContent).toContain("Waiting for dispute window");
    m.click("Dispute");
    // PAY-13: the window is said in time against the chain's clock, never in blocks.
    const d = document.querySelector('[role="dialog"]')!;
    expect(d.textContent).toContain("ends 2026-11-20 12:00 UTC, in about 2 h");
    expect(d.textContent).not.toMatch(/block/);
    expect(d.textContent).toContain("A dispute pays the disputer nothing.");
    m.done();
  });
  it("Success: Claimable → the claim review in SALT only → handed to the wallet review", async () => {
    const m = mount(await open(), { receipts: [{ line, status: "claimable" }], pullWei: 0n });
    expect(m.el.textContent).toContain("7 units × 1.50 SALT = 10.50 SALT");
    m.click("Claim");
    const d = document.querySelector('[role="dialog"]')!;
    expect(d.textContent).toContain("HIC-1");
    expect(d.textContent).toContain("Your WorkMarket pull balance, with your join bond released, then withdraw to your wallet");
    expect(d.textContent).not.toMatch(/USD|\$|earn/);
    m.click("Sign");
    await act(async () => {});
    expect(m.actFn).toHaveBeenCalledWith("claim", { line, status: "claimable" });
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    m.done();
  });
  it("Claimed and Disputed, frozen show their words; a positive pull balance can be withdrawn", async () => {
    const m = mount(await open(), { receipts: [{ line, status: "claimed" }, { line: { ...line, round: "15" }, status: "disputed" }], pullWei: (21n * SALT) / 2n });
    expect(m.el.textContent).toContain("Claimed");
    expect(m.el.textContent).toContain("Disputed, frozen");
    expect(m.el.textContent).toContain("10.50 SALT");
    m.click("Withdraw to wallet");
    expect(document.querySelector('[role="dialog"]')!.textContent).toContain("Withdraw to your wallet");
    m.done();
  });
  it("PAY-15: a second press while signing hands nothing over", async () => {
    let release!: () => void;
    const m = mount(await open(), { receipts: [{ line, status: "claimable" }], pullWei: 0n, act: () => new Promise<void>((r) => (release = r)) });
    m.click("Claim");
    m.click("Sign");
    expect([...document.querySelectorAll('[role="dialog"] button')].map((b) => b.textContent)).not.toContain("Sign");
    expect(document.querySelector('[role="status"]')!.textContent).toContain("Reviewed by you");
    await act(async () => release());
    expect(m.actFn).toHaveBeenCalledTimes(1);
    m.done();
  });
  it("Failure: a refused hand-off is Not sent with the reason", async () => {
    const m = mount(await open(), { receipts: [{ line, status: "claimable" }], pullWei: 0n, act: async () => Promise.reject(new Error("another approval is already waiting")) });
    m.click("Claim");
    m.click("Sign");
    await act(async () => {});
    expect(document.querySelector('[role="alert"]')!.textContent).toBe("Not sent: another approval is already waiting");
    m.done();
  });
});

describe("History", () => {
  it("lists the device's records newest first, from core's fl_rounds.rs overview", () => {
    const rows = historyRows(ov);
    expect(rows.map((r) => r.kind)).toEqual(["Gate", "Consent", "Round", "Consent"]);
    expect(rows[0].what).toBe("ACCEPT · no metric worse, mean +0.019");
    expect(rows[1].what).toMatch(/^Withdrew consent for round 0x3e3e/);
    expect(filterRows(rows, "Consent").length).toBe(2);
    expect(historyCsv(rows).split("\n")[0]).toBe("when,kind,what,record");
  });
  it("filters and exports; empty says so", async () => {
    const m = mount(await nd());
    m.click("History");
    expect(m.el.querySelectorAll('[data-testid="history"] tbody tr').length).toBe(4);
    m.click("Gate");
    expect(m.el.querySelectorAll('[data-testid="history"] tbody tr').length).toBe(1);
    m.click("Export as CSV");
    await act(async () => {});
    expect(m.el.textContent).toContain("Saved citrate-fl-history.csv to Downloads");
    m.done();
    const e = mount(await nd(), { ov: { ...ov, starts: [], gates: [] } });
    e.click("History");
    expect(e.el.querySelector('[data-testid="history-empty"]')).not.toBeNull();
    e.done();
  });
});

describe("#695 review: banned words", async () => {
  const BANNED = /\b(earn|earns|earned|earning|earnings|income|projected|estimate|yield|APY|APR|profit|investment|USD|dormant|Final|Settled|HITL|users?)\b|!|\$/;
  for (const [name, make] of [
    ["not deployed", nd],
    ["dark", dark],
    ["open", open],
  ] as const) {
    it(`${name}: receipts, history and every review sheet pass`, async () => {
      const m = mount(await make(), { receipts: [{ line, status: "claimable" }, { line: { ...line, round: "15" }, status: "waiting", disputeEndsAt: 1 }], pullWei: 3n * SALT });
      const texts = [m.el.textContent ?? ""];
      for (const b of ["Claim", "Dispute", "Withdraw to wallet"]) {
        const btn = [...document.querySelectorAll("button")].find((x) => x.textContent?.trim() === b && !(x as HTMLButtonElement).disabled);
        if (!btn) continue;
        act(() => btn.click());
        texts.push(document.querySelector('[role="dialog"]')?.textContent ?? "");
        m.click("Decline");
      }
      m.click("History");
      texts.push(m.el.textContent ?? "");
      for (const t of texts) expect(t).not.toMatch(BANNED);
      m.done();
    });
  }
});
