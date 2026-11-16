// FL design board 11, Settings › Work: one test per StateMatrix state (Empty: no terms yet; Not
// deployed: floor not enforced yet; Pending: the confirm card; Success: saved and sealed, envelope
// on; Failure: save failed, nothing changed; Unavailable: a failed read is said), plus consent and
// the envelope's approval card. Core is faked through the view's seams in fl_work.rs's shape.
import { describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import realBook from "../../src-tauri/addresses/40204.json";
import type { FlOverview, FlRoundsDomain } from "../bridge/domains";
import { readFlNetworkState } from "./networkState";
import type { EnvelopeRecord } from "./authority";
import { SETTINGS_WORK_ROUTE, WorkSettingsView } from "./WorkSettings";
import type { WorkerTerms } from "./workerTerms";
import { locate, registerFor } from "../shell/places";

const reads = { codeSize: async () => 1000, viewCall: async () => "0x" + "0".repeat(64) };
const ov = (rounds: string[] = []): FlOverview => ({ config: { url: null, source: "none", settingsUrl: null, note: null }, starts: [], gates: [], activeAdapter: null, rememberedAdapter: null, restoreError: null, consentedRounds: rounds, consentFile: null, consentError: null, storeError: null });
const fl = (o = ov()): FlRoundsDomain =>
  ({ overview: vi.fn(async () => o), revokeConsent: vi.fn(async () => []), trajectoryStatus: vi.fn(async () => ({ settings: { enabled: false, changedAtMs: null }, recordedFiles: 0, datasets: 0, appliesOnRestart: false, loadError: null })) }) as unknown as FlRoundsDomain;
const saved: WorkerTerms = { floorSaltPerUnit: "1.20", hours: { start: "23:00", end: "07:00" }, weekdaysOnly: true, mainsOnly: true, dataClasses: ["support-tickets"] };
const ENV: EnvelopeRecord = { on: true, name: "Weeknight rounds", joinsPerWeek: 3, maxHoursPerRound: 6, approvedAt: Date.UTC(2026, 10, 15, 21, 25), offAt: null, weekStart: 0, joinsThisWeek: 2 };

async function mount(o: {
  load?: () => Promise<WorkerTerms | null>;
  save?: (t: WorkerTerms) => Promise<{ savedAt: number; termsHash: string; sealed: boolean }>;
  read?: () => Promise<EnvelopeRecord | null>;
  approve?: () => Promise<EnvelopeRecord>;
  off?: () => Promise<EnvelopeRecord | null>;
  sig?: string;
  domain?: FlRoundsDomain;
} = {}) {
  const state = await readFlNetworkState(realBook, reads);
  const el = document.createElement("div");
  document.body.appendChild(el);
  const root = createRoot(el);
  const save = vi.fn(o.save ?? (async () => ({ savedAt: Date.UTC(2026, 10, 15, 21, 22), termsHash: "ab".repeat(32), sealed: true })));
  const approve = vi.fn(o.approve ?? (async () => ENV));
  const off = vi.fn(o.off ?? (async () => ({ ...ENV, on: false, offAt: 9 })));
  const requestSig = vi.fn(async () => o.sig ?? "approved");
  const domain = o.domain ?? fl();
  await act(async () =>
    root.render(
      <WorkSettingsView
        state={state}
        fl={domain}
        wallet={"0x5b02" + "0".repeat(32) + "19fe"}
        requestSig={requestSig}
        toast={() => {}}
        go={() => {}}
        terms={{ load: o.load ?? (async () => null), save }}
        envelope={{ read: o.read ?? (async () => null), approve, off }}
      />,
    ),
  );
  const click = async (t: string) => {
    const b = [...el.querySelectorAll("button")].find((x) => x.textContent?.trim().endsWith(t));
    if (!b) throw new Error("no button " + t);
    await act(async () => b.click());
  };
  return { el, click, save, approve, off, requestSig, domain, done: () => (act(() => root.unmount()), el.remove()) };
}

describe("Settings › Work route", () => {
  it("is a Settings tab on Charter, at the route lanes link to", () => {
    expect(SETTINGS_WORK_ROUTE).toBe("settings/work");
    expect(locate("settings/work")).toMatchObject({ place: "settings", tab: "settings/work" });
    expect(registerFor("settings/work")).toBe("charter");
  });
});

describe("Terms (StateMatrix)", () => {
  it("Empty: No terms yet; Not deployed: Floor not enforced yet", async () => {
    const m = await mount();
    expect(m.el.textContent).toContain("No terms yet.");
    expect(m.el.textContent).toContain("Floor not enforced yet: WorkMarket is not available yet on this network.");
    m.done();
  });
  it("Pending then Success: the confirm card, then Saved and sealed with the hash", async () => {
    const m = await mount({ load: async () => saved });
    expect((m.el.querySelector('[data-testid="terms-floor"]') as HTMLInputElement).value).toBe("1.20");
    await m.click("Save terms");
    const card = m.el.querySelector('[data-testid="terms-confirm"]')!;
    expect(card.textContent).toContain("HIC-1");
    expect(card.textContent).toContain("Floor 1.20 SALT · 23:00 to 07:00 · weekdays · mains only · support-tickets.");
    expect(m.save).not.toHaveBeenCalled();
    await m.click("Confirm");
    expect(m.save).toHaveBeenCalledWith(saved);
    expect(m.el.textContent).toMatch(/Saved and sealed at \d\d:\d\d · 0xabab/);
    m.done();
  });
  it("Failure: Save failed, nothing changed, with core's reason", async () => {
    const m = await mount({ save: async () => Promise.reject(new Error("the keychain is not reachable; your work settings stay closed")) });
    await m.click("Save terms");
    await m.click("Confirm");
    expect(m.el.querySelector('[role="status"]')!.textContent).toBe("Save failed, nothing changed: the keychain is not reachable; your work settings stay closed");
    m.done();
  });
  it("Unavailable: a failed read is said, and Save waits for nothing invented", async () => {
    const m = await mount({ load: async () => Promise.reject(new Error("terms.sealed does not open on this device")) });
    expect(m.el.querySelector('[role="alert"]')!.textContent).toContain("terms.sealed does not open on this device");
    m.done();
  });
  it("malformed terms cannot be saved", async () => {
    const m = await mount();
    const i = m.el.querySelector<HTMLInputElement>('[data-testid="terms-floor"]')!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(i, "abc");
      i.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect([...m.el.querySelectorAll("button")].find((b) => b.textContent === "Save terms")!.disabled).toBe(true);
    expect(m.el.textContent).toContain("The floor must be a SALT amount like 1.20");
    m.done();
  });
});

describe("Consent", () => {
  it("lists per-round consent with Withdraw (no card), and the conversations switch", async () => {
    const R = "0x" + "3e".repeat(32);
    const m = await mount({ domain: fl(ov([R])) });
    await m.click("Consent");
    expect(m.el.textContent).toContain("Consented");
    await m.click("Withdraw");
    expect(m.domain.revokeConsent).toHaveBeenCalledWith(R);
    expect(m.requestSig).not.toHaveBeenCalled();
    expect(m.el.querySelector('[data-testid="fl-trajectories"]')).not.toBeNull();
    m.done();
  });
  it("no consent says so", async () => {
    const m = await mount();
    await m.click("Consent");
    expect(m.el.querySelector('[data-testid="consent-none"]')).not.toBeNull();
    m.done();
  });
});

describe("Envelope (HIC-2)", () => {
  it("Off by default; approving goes through the approval card, then it is on", async () => {
    const m = await mount();
    await m.click("Envelope (HIC-2)");
    expect(m.el.querySelector('[data-testid="env-state"]')!.textContent).toContain("Every join asks you.");
    await act(async () => m.el.querySelector<HTMLButtonElement>('[aria-label="Raise joins per week"]')!.click());
    await m.click("Approve this envelope");
    expect(m.requestSig).toHaveBeenCalledTimes(1);
    const spec = (m.requestSig.mock.calls[0] as unknown[])[0] as { chainless: boolean; rows: { k: string; v: string }[] };
    expect(spec.chainless).toBe(true);
    expect(spec.rows.find((r) => r.k === "Joins per week")!.v).toBe("4");
    expect(m.approve).toHaveBeenCalledWith({ name: "Weeknight rounds", joinsPerWeek: 4, maxHoursPerRound: 6 });
    expect(m.el.querySelector('[data-testid="env-state"]')!.textContent).toContain("On");
    m.done();
  });
  it("declining the card changes nothing", async () => {
    const m = await mount({ sig: "rejected" });
    await m.click("Envelope (HIC-2)");
    await m.click("Approve this envelope");
    expect(m.approve).not.toHaveBeenCalled();
    expect(m.el.textContent).toContain("Declined. Nothing changed; every join still asks you.");
    m.done();
  });
  it("on: shows what is left; Turn off now needs no card", async () => {
    const m = await mount({ read: async () => ENV });
    await m.click("Envelope (HIC-2)");
    expect(m.el.textContent).toContain("3 joins per week (1 left this week), up to 6 machine hours per round.");
    await m.click("Turn off now");
    expect(m.off).toHaveBeenCalled();
    expect(m.requestSig).not.toHaveBeenCalled();
    expect(m.el.textContent).toContain("Every join asks you.");
    m.done();
  });
  it("a failed envelope read is said; Hermes stays on HIC-1", async () => {
    const m = await mount({ read: async () => Promise.reject(new Error("envelope.sealed does not open on this device")) });
    await m.click("Envelope (HIC-2)");
    expect(m.el.textContent).toContain("envelope.sealed does not open on this device");
    expect(m.el.textContent).toContain("Hermes uses HIC-1 until it is read.");
    m.done();
  });
});

describe("Payout address", () => {
  it("is the member's wallet", async () => {
    const m = await mount();
    await m.click("Payout address");
    expect(m.el.textContent).toContain("Payouts land in your WorkMarket pull balance");
    m.done();
  });
});
