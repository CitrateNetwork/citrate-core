// FL design board 09, Rounds › Adapters: one test per StateMatrix state (Empty: scorecards needed;
// Not deployed: download not available; Pending: comparing; Success: ACCEPT and loaded; Failure:
// REJECT, Not comparable, Refused with the named check; Unavailable: re-apply failed). The gate
// records here are TEST-ONLY fixtures in core's fl_rounds.rs shape.
import { describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { FlAdapterGateRecord, FlOverview, FlRoundsDomain } from "../bridge/domains";
import { AdaptersView, checkStates, refusalCheck, verdictOf } from "./Adapters";

const SHA = "a77d".padEnd(64, "0");
const overview = (over: Partial<FlOverview> = {}): FlOverview => ({
  config: { url: null, source: "none", settingsUrl: null, note: null },
  starts: [],
  gates: [],
  activeAdapter: null,
  rememberedAdapter: null,
  restoreError: null,
  consentedRounds: [],
  consentFile: null,
  consentError: null,
  storeError: null,
  ...over,
});
const record = (verdict: "ACCEPT" | "REJECT", reasons: string[] = [], round = true): FlAdapterGateRecord => ({
  adapterSha256: SHA,
  adapterPath: "/m/a.gguf",
  baseModel: "llama-8b.gguf",
  decidedAtMs: 1,
  round: round ? { roundId: "0x" + "3e".repeat(32), recordDigest: "0x" + "11".repeat(32), adapterSha256: SHA, chainId: 40204, ledger: "0x" + "25".repeat(20), participants: 5 } : null,
  decision: {
    verdict,
    reasons,
    metrics: [
      { metric: "tool-call accuracy", base: 0.812, candidate: 0.829, improvement: 0.017 },
      { metric: "QA exact match", base: 0.644, candidate: verdict === "ACCEPT" ? 0.661 : 0.63, improvement: verdict === "ACCEPT" ? 0.017 : -0.014 },
    ],
    compositeBase: 0.728,
    compositeCandidate: verdict === "ACCEPT" ? 0.745 : 0.729,
  },
});

function domain(over: Partial<FlRoundsDomain> = {}): FlRoundsDomain {
  return {
    overview: vi.fn(async () => overview()),
    setCoordinator: vi.fn(),
    plan: vi.fn(),
    lookupPlan: vi.fn(),
    start: vi.fn(),
    gateAdapter: vi.fn(async () => record("ACCEPT")),
    loadAdapter: vi.fn(async () => "/served/a.gguf"),
    unloadAdapter: vi.fn(async () => {}),
    revokeConsent: vi.fn(),
    trajectoryStatus: vi.fn(),
    setTrajectoryConsent: vi.fn(),
    buildTrainingSet: vi.fn(),
    ...over,
  } as unknown as FlRoundsDomain;
}

async function mount(fl: FlRoundsDomain) {
  const el = document.createElement("div");
  document.body.appendChild(el);
  const root = createRoot(el);
  await act(async () => root.render(<AdaptersView fl={fl} toast={() => {}} go={() => {}} pick={async () => null} />));
  const type = (k: string, v: string) => {
    const i = el.querySelector<HTMLInputElement>(`[data-testid="gate-${k}"]`)!;
    act(() => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      set.call(i, v);
      i.dispatchEvent(new Event("input", { bubbles: true }));
    });
  };
  const fill = () => {
    type("adapter", "/m/a.gguf");
    type("round", "/m/round.json");
    type("baseTools", "/m/base.json");
    type("candTools", "/m/cand.json");
  };
  const run = async () => {
    await act(async () => el.querySelector<HTMLButtonElement>('[data-testid="gate-run"]')!.click());
  };
  const done = () => {
    act(() => root.unmount());
    el.remove();
  };
  return { el, fill, run, done, type };
}

describe("refusal to check mapping (core fl_rounds.rs messages)", () => {
  it("names the check each refusal belongs to", () => {
    expect(refusalCheck("the adapter's sha256 ab does not match the expected cd")).toBe("sha");
    expect(refusalCheck("the round is Rejected, not Accepted")).toBe("accepted");
    expect(refusalCheck("the round had 2 devices; at least 3 are required")).toBe("devices");
    expect(refusalCheck("the round's record digests disagree (chain, bundle and replay must match)")).toBe("digests");
    expect(refusalCheck("the independent replay found 1 mismatch(es) in this round")).toBe("replay");
    expect(refusalCheck("the round result names more than one round")).toBe("ids");
    expect(refusalCheck("cannot open /x: No such file")).toBeNull();
  });
  it("checks before the refused one passed; later ones did not run", () => {
    const s = checkStates({ record: null, refusal: "the round had 2 devices; at least 3 are required", withRound: true });
    expect(s).toEqual({ ids: "passed", digests: "passed", replay: "passed", devices: "failed", accepted: "not-run", sha: "not-run" });
    const file = checkStates({ record: record("ACCEPT", [], false), refusal: null, withRound: false });
    expect(file.sha).toBe("passed");
    expect(file.accepted).toBe("not-applicable");
  });
  it("verdicts", () => {
    expect(verdictOf(null, null)).toBe("Scorecards needed");
    expect(verdictOf(record("ACCEPT"), null)).toBe("ACCEPT");
    expect(verdictOf(record("REJECT", ["QA exact match got worse (0.6440 to 0.6300)"]), null)).toBe("REJECT");
    expect(verdictOf(record("REJECT", ["tool-call eval: the runs used different datasets (a n=400 vs a n=380)"]), null)).toBe("Not comparable");
    expect(verdictOf(null, "x")).toBe("Refused");
  });
});

describe("Adapters states (StateMatrix)", () => {
  it("Empty: Scorecards needed, Load disabled; Not deployed: download not available", async () => {
    const m = await mount(domain());
    expect(m.el.textContent).toContain("Scorecards needed");
    expect(m.el.textContent).toContain("Adapter download from LoRAFactory is not built");
    expect(m.el.querySelector<HTMLButtonElement>('[data-testid="gate-load"]')!.disabled).toBe(true);
    expect(m.el.querySelector<HTMLButtonElement>('[data-testid="gate-run"]')!.disabled).toBe(true);
    expect(m.el.textContent).toContain("Not loaded");
    m.done();
  });
  it("Pending: Comparing while core runs the gate", async () => {
    let release!: (r: FlAdapterGateRecord) => void;
    const fl = domain({ gateAdapter: vi.fn(() => new Promise<FlAdapterGateRecord>((res) => (release = res))) });
    const m = await mount(fl);
    m.fill();
    await m.run();
    expect(m.el.textContent).toContain("Comparing");
    await act(async () => release(record("ACCEPT")));
    m.done();
  });
  it("Success: ACCEPT with the gate table, then Loaded on the base model", async () => {
    let active: string | null = null;
    const fl = domain({
      overview: vi.fn(async () => overview(active ? { activeAdapter: active, rememberedAdapter: { sha256: SHA, baseModel: "llama-8b.gguf" } } : {})),
      loadAdapter: vi.fn(async () => {
        active = "/served/a.gguf";
        return active;
      }),
    });
    const m = await mount(fl);
    m.fill();
    await m.run();
    expect(fl.gateAdapter).toHaveBeenCalledWith({ adapterPath: "/m/a.gguf", expectedSha256: "", baseToolsPath: "/m/base.json", candidateToolsPath: "/m/cand.json", roundResultPath: "/m/round.json" });
    expect(m.el.querySelector('[data-testid="gate-table"]')).not.toBeNull();
    expect(m.el.textContent).toContain("ACCEPT: no metric got worse and the mean of shared metrics rose by 0.017.");
    expect(m.el.textContent).toContain("the round result says Accepted");
    expect([...m.el.querySelectorAll("[data-check]")].every((c) => c.getAttribute("data-state") === "passed")).toBe(true);
    const load = m.el.querySelector<HTMLButtonElement>('[data-testid="gate-load"]')!;
    expect(load.disabled).toBe(false);
    await act(async () => load.click());
    expect(fl.loadAdapter).toHaveBeenCalledWith(SHA);
    expect(m.el.querySelector('[data-testid="apply-state"]')!.textContent).toContain("Loaded on llama-8b.gguf");
    m.done();
  });
  it("Failure: REJECT names the metric that got worse, and Load stays off", async () => {
    const fl = domain({ gateAdapter: vi.fn(async () => record("REJECT", ["QA exact match got worse (0.6440 to 0.6300)"])) });
    const m = await mount(fl);
    m.fill();
    await m.run();
    expect(m.el.querySelector('[data-result="got worse"]')!.textContent).toContain("QA exact match");
    expect(m.el.textContent).toContain("REJECT: QA exact match got worse");
    expect(m.el.querySelector<HTMLButtonElement>('[data-testid="gate-load"]')!.disabled).toBe(true);
    m.done();
  });
  it("Failure: Not comparable says what differs", async () => {
    const fl = domain({ gateAdapter: vi.fn(async () => record("REJECT", ["tool-call eval: the runs used different datasets (v3 n=400 vs v3 n=380)"])) });
    const m = await mount(fl);
    m.fill();
    await m.run();
    expect(m.el.textContent).toContain("Not comparable: tool-call eval: the runs used different datasets");
    m.done();
  });
  it("Failure: Refused names the check that refused it", async () => {
    const fl = domain({ gateAdapter: vi.fn(async () => Promise.reject(new Error("the round is Rejected, not Accepted"))) });
    const m = await mount(fl);
    m.fill();
    await m.run();
    expect(m.el.querySelector('[data-testid="gate-refused"]')!.textContent).toContain("Refused before the gate: the round is Rejected, not Accepted");
    expect(m.el.querySelector('[data-check="accepted"]')!.getAttribute("data-state")).toBe("failed");
    expect(m.el.querySelector('[data-check="sha"]')!.getAttribute("data-state")).toBe("not-run");
    m.done();
  });
  it("Unavailable: Re-apply failed, and the model still started", async () => {
    const m = await mount(domain({ overview: vi.fn(async () => overview({ restoreError: "the base model changed" })) }));
    expect(m.el.querySelector('[data-testid="apply-state"]')!.textContent).toContain("Re-apply failed");
    expect(m.el.textContent).toContain("The model started without the adapter: the base model changed");
    m.done();
  });
  it("a local file has no round checks to run", async () => {
    const fl = domain({ gateAdapter: vi.fn(async () => record("ACCEPT", [], false)) });
    const m = await mount(fl);
    act(() => [...m.el.querySelectorAll("button")].find((b) => b.textContent === "A local file")!.click());
    m.type("adapter", "/m/a.gguf");
    m.type("sha", SHA);
    m.type("baseTools", "/m/b.json");
    m.type("candTools", "/m/c.json");
    await m.run();
    expect((fl.gateAdapter as ReturnType<typeof vi.fn>).mock.calls[0][0].roundResultPath).toBeUndefined();
    expect(m.el.querySelector('[data-check="accepted"]')!.getAttribute("data-state")).toBe("not-applicable");
    expect(m.el.textContent).toContain("No round result was given");
    m.done();
  });
});
