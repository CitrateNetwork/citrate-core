// K2 (0.5.2): Hermes › Controls, third depth ("3 · Records"). What happened, read from the stores
// that already exist: approval decisions, the escalation spend ledger and each daemon's run log.
// Every number comes from a read; a failed read shows the call that failed and never a zero.
// Money copy (UI truth contract §5, chain-team rulings 4 and 5): dollar amounts are the provider's
// list price on the member's own keys, never next to SALT; daemons say "Daemons can't spend SALT".
import { describe, it, expect, afterEach, beforeEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ControlsRecords, RUN_WINDOW_DAYS, openRecords, recordsSlice, type RecordsIo } from "./ControlsRecords";
import type { EscalationBudget } from "../../bridge/domains";
import type { DaemonRunEntry, DaemonRunLog } from "../../daemons/api";
import type { WebBudgetStatus } from "../../budgets/budgets";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});
beforeEach(() => {
  recordsSlice.set({ view: "approvals", daemonId: null });
});

const $ = (id: string) => host?.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
const $$ = (id: string) => Array.from(host?.querySelectorAll(`[data-testid="${id}"]`) ?? []) as HTMLElement[];
const flush = () => act(async () => await new Promise((r) => setTimeout(r, 0)));
const text = () => host?.textContent ?? "";

const NOW = Date.UTC(2026, 9, 19, 15, 0, 0);

const budget = (over: Partial<EscalationBudget> = {}): EscalationBudget => ({
  capMicros: 0,
  usedMicros: 0,
  remainingMicros: 0,
  confirmedMicros: 0,
  periodStartMs: NOW - 3_600_000,
  periodEndMs: NOW + 3_600_000,
  maxCapMicros: 100_000_000,
  unreadable: false,
  history: [],
  ...over,
});

const run = (over: Partial<DaemonRunEntry> = {}): DaemonRunEntry => ({
  schema: 1,
  daemonId: "d1",
  runId: "r1",
  name: "Morning digest",
  startedMs: NOW - 60_000,
  endedMs: NOW - 30_000,
  tokens: 1234,
  tokenSource: "measured",
  outcome: "answered",
  ...over,
});

const webStatus = (records: unknown[]): WebBudgetStatus =>
  ({
    snapshot: { health: { state: "ok" }, budgets: [], records, headHash: "0x00", recordCount: records.length },
    attestation: { available: false, reason: "" },
    defaults: { maxCount: 3, ttlDays: 7, pendingOwnerSignoff: true },
    ceilings: { maxCount: 10, ttlDays: 30 },
    rate: { minGapSeconds: 60, windowMax: 10, windowHours: 24 },
    walletAddress: null,
    nowMs: NOW,
  }) as WebBudgetStatus;

function io(o: Partial<RecordsIo> = {}): RecordsIo {
  return {
    escalationBudget: vi.fn(async () => budget()),
    webSignIn: vi.fn(async () => webStatus([])),
    runsBetween: vi.fn(async (): Promise<DaemonRunLog> => ({ runs: [], unreadable: 0 })),
    now: () => NOW,
    ...o,
  };
}

async function mount(i: RecordsIo) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<ControlsRecords io={i} />);
  });
  await flush();
}
async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await flush();
}

describe("Controls › 3 · Records: approval records", () => {
  it("names the missing read command for Hermes approval records instead of showing an empty list", async () => {
    await mount(io());
    const gap = $("hic-records-gap");
    expect(gap).toBeTruthy();
    expect(gap?.getAttribute("data-gap")).toBe("hic-records-read");
    expect(gap?.textContent).toContain("Not available yet");
    expect(gap?.textContent).toContain("hic_record_decision");
    // Not drawn as "no decisions": the records exist on this machine, they just can't be read here.
    expect(text()).not.toContain("No decisions recorded");
  });

  it("lists the web sign-in decision records core keeps, newest first", async () => {
    const i = io({
      webSignIn: vi.fn(async () =>
        webStatus([
          { record_id: 1, kind: "budget_granted", budget_id: 1, principal: "0xabc", origin: "https://a.example", at_ms: NOW - 7200_000, prev_hash: "0x0", hash: "0x1", status: "final" },
          { record_id: 2, kind: "auto_sign", budget_id: 1, principal: "0xabc", origin: "https://a.example", at_ms: NOW - 3600_000, prev_hash: "0x1", hash: "0x2", status: "signed" },
        ]),
      ),
    });
    await mount(i);
    const rows = $$("web-record");
    expect(rows.length).toBe(2);
    expect(rows[0].textContent).toContain("#2");
    expect(rows[0].textContent).toContain("signed");
    expect(rows[1].textContent).toContain("#1");
    expect(rows[1].textContent).toContain("https://a.example");
  });

  it("a failed web sign-in read names web_budget_status and shows no rows", async () => {
    await mount(io({ webSignIn: vi.fn(async () => Promise.reject(new Error("store locked"))) }));
    expect($("web-records-error")?.textContent).toContain("web_budget_status");
    expect($("web-records-error")?.textContent).toContain("store locked");
    expect($$("web-record").length).toBe(0);
  });

  it("outside the desktop app it says so", async () => {
    await mount(io({ webSignIn: null, runsBetween: null }));
    expect($("web-records-unavailable")?.textContent).toContain("desktop app");
  });
});

describe("Controls › 3 · Records: spend ledger", () => {
  it("shows the cap, today's use and confirmed spend from escalation_budget at the provider's list price, and no SALT amount", async () => {
    const i = io({
      escalationBudget: vi.fn(async () =>
        budget({
          capMicros: 1_000_000,
          usedMicros: 250_000,
          confirmedMicros: 120_000,
          history: [
            { escalationId: "e1", endpointId: "p1", destination: "api.anthropic.com · model-x", quotedMicros: 300_000, chargedMicros: 250_000, mode: "budget", outcome: "answered", usageReported: true, exceededQuote: false, atMs: NOW - 600_000 },
            { escalationId: "e2", endpointId: "p1", destination: "api.anthropic.com · model-x", quotedMicros: 150_000, chargedMicros: 120_000, mode: "confirmed", outcome: "answered", usageReported: true, exceededQuote: false, atMs: NOW - 300_000 },
          ],
        }),
      ),
    });
    await mount(i);
    await click($("records-nav-spend"));
    expect($("spend-cap")?.textContent).toContain("$1.00");
    expect($("spend-used")?.textContent).toContain("$0.25");
    expect($("spend-confirmed")?.textContent).toContain("$0.12");
    const rows = $$("spend-row");
    expect(rows.length).toBe(2);
    // Newest first.
    expect(rows[0].textContent).toContain("you approved");
    expect(rows[1].textContent).toContain("within the cap");
    expect(text()).toContain("list price");
    expect(text()).toContain("your own key");
    expect(text()).toContain("Daemons can’t spend SALT");
    // Ruling 5: never "0 SALT"; §5: no SALT amount anywhere on this view.
    expect(text()).not.toMatch(/\d\s*SALT/);
  });

  it("at a $0.00 cap with no history it says every escalation asks, without a ledger table", async () => {
    await mount(io());
    await click($("records-nav-spend"));
    expect($("spend-cap")?.textContent).toContain("$0.00");
    expect($("spend-empty")?.textContent).toContain("asks you first");
    expect($$("spend-row").length).toBe(0);
  });

  it("a failed read shows the unavailable state, not $0.00", async () => {
    await mount(io({ escalationBudget: vi.fn(async () => Promise.reject(new Error("no keyring"))) }));
    await click($("records-nav-spend"));
    expect($("spend-error")?.textContent).toContain("escalation_budget");
    expect($("spend-error")?.textContent).toContain("no keyring");
    expect($("spend-cap")).toBeNull();
    expect(text()).not.toContain("$0.00");
  });

  it("an unreadable ledger file says so and shows no spend figures", async () => {
    await mount(io({ escalationBudget: vi.fn(async () => budget({ unreadable: true })) }));
    await click($("records-nav-spend"));
    expect($("spend-unreadable")?.textContent).toContain("could not be read");
    expect($("spend-used")).toBeNull();
  });
});

describe("Controls › 3 · Records: daemon run history", () => {
  it("reads the run log for the last week from daemon_runs_between and lists runs newest first", async () => {
    const runsBetween = vi.fn(async (): Promise<DaemonRunLog> => ({
      runs: [run({ runId: "a", startedMs: NOW - 90_000, tokens: 800, tokenSource: "estimated", outcome: "over_budget" }), run({ runId: "b", startedMs: NOW - 30_000 })],
      unreadable: 0,
    }));
    await mount(io({ runsBetween }));
    await click($("records-nav-runs"));
    expect(runsBetween).toHaveBeenCalledWith(NOW - RUN_WINDOW_DAYS * 86_400_000, NOW + 1);
    const rows = $$("run-row");
    expect(rows.length).toBe(2);
    expect(rows[0].textContent).toContain("answered");
    expect(rows[0].textContent).toContain("1,234");
    expect(rows[0].textContent).toContain("measured");
    expect(rows[1].textContent).toContain("over budget");
    expect(rows[1].textContent).toContain("estimated");
  });

  it("filters by daemon, and opens on the daemon another view asked for", async () => {
    const runsBetween = vi.fn(async (): Promise<DaemonRunLog> => ({
      runs: [run({ runId: "a", daemonId: "d1", name: "Morning digest" }), run({ runId: "b", daemonId: "d2", name: "Inbox sweep" })],
      unreadable: 0,
    }));
    openRecords("runs", "d2");
    await mount(io({ runsBetween }));
    expect($$("run-row").length).toBe(1);
    expect($$("run-row")[0].textContent).toContain("Inbox sweep");
    await click($("runs-pick-all"));
    expect($$("run-row").length).toBe(2);
    await click($("runs-pick-d1"));
    expect($$("run-row").length).toBe(1);
    expect($$("run-row")[0].textContent).toContain("Morning digest");
  });

  it("counts unreadable lines instead of guessing them", async () => {
    await mount(io({ runsBetween: vi.fn(async () => ({ runs: [], unreadable: 3 })) }));
    await click($("records-nav-runs"));
    expect($("runs-unreadable")?.textContent).toContain("3");
  });

  it("a failed read names daemon_runs_between and shows no rows", async () => {
    await mount(io({ runsBetween: vi.fn(async () => Promise.reject(new Error("metering folder missing"))) }));
    await click($("records-nav-runs"));
    expect($("runs-error")?.textContent).toContain("daemon_runs_between");
    expect($("runs-error")?.textContent).toContain("metering folder missing");
    expect($$("run-row").length).toBe(0);
  });

  it("outside the desktop app it says daemons run in the desktop app", async () => {
    await mount(io({ runsBetween: null }));
    await click($("records-nav-runs"));
    expect($("runs-unavailable")?.textContent).toContain("desktop app");
  });
});
