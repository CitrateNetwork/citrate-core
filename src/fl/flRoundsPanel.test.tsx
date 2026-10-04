// HUP-S9.4 — the federated-rounds panel on the Train surface. Written red-first (the component did
// not exist). The domain is a typed in-test double of FlRoundsDomain; production uses core.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { FlAdapterGateRecord, FlOverview, FlRoundsDomain } from "../bridge/domains";
import { FlRoundsPanel } from "./FlRoundsPanel";
import { livePlan, PLAN_HASH } from "./fixtures/plan";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function overview(over: Partial<FlOverview> = {}): FlOverview {
  return {
    config: { url: null, source: "none", settingsUrl: null, note: null },
    starts: [],
    gates: [],
    activeAdapter: null,
    rememberedAdapter: null,
    restoreError: null,
    consentedRounds: [],
    consentFile: "/data/fl_consent.json",
    consentError: null,
    storeError: null,
    ...over,
  };
}

function gateRec(verdict: "ACCEPT" | "REJECT"): FlAdapterGateRecord {
  return {
    adapterSha256: "c".repeat(64),
    adapterPath: "/x/a.gguf",
    baseModel: "m",
    decidedAtMs: 1,
    round: null,
    decision: {
      verdict,
      reasons: verdict === "REJECT" ? ["the eval score did not improve (0.8000 to 0.8000)"] : [],
      metrics: [],
      compositeBase: 0.8,
      compositeCandidate: verdict === "ACCEPT" ? 0.9 : 0.8,
    },
  };
}

function domain(over: Partial<FlRoundsDomain> = {}): FlRoundsDomain {
  return {
    overview: vi.fn(async () => overview()),
    setCoordinator: vi.fn(async (url: string | null) => ({ url, source: url ? "settings" : "none", settingsUrl: url, note: null }) as const),
    plan: vi.fn(async () => livePlan()),
    lookupPlan: vi.fn(async () => livePlan()),
    start: vi.fn(async (h: string) => ({ planHash: h, coordinatorUrl: "https://coordinator.example.org", authorizedAtMs: 2, trainingStarted: false, note: "This build does not include the device training worker, so no training has started." })),
    gateAdapter: vi.fn(async () => gateRec("ACCEPT")),
    loadAdapter: vi.fn(async () => "/data/adapters/" + "c".repeat(64) + ".gguf"),
    unloadAdapter: vi.fn(async () => {}),
    revokeConsent: vi.fn(async () => []),
    ...over,
  };
}

async function mount(el: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(el);
  });
  await act(async () => {});
  return { host, root };
}
const q = <T extends Element = HTMLElement>(host: HTMLElement, id: string) => host.querySelector(`[data-testid="${id}"]`) as T | null;
async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await act(async () => {});
}
async function type(el: Element | null, value: string) {
  expect(el).toBeTruthy();
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  await act(async () => {
    setter?.call(el, value);
    el!.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("FlRoundsPanel — coordinator", () => {
  it("says honestly that no coordinator is configured and that live rounds need one", async () => {
    const { host } = await mount(<FlRoundsPanel fl={domain()} requestSig={vi.fn()} toast={vi.fn()} />);
    expect(q(host, "fl-coordinator")!.textContent).toMatch(/No training coordinator is configured/);
    expect(q(host, "fl-coordinator")!.textContent).toMatch(/Live rounds need/);
  });

  it("saves a coordinator URL through core and shows core's error for a bad one", async () => {
    const fl = domain({
      setCoordinator: vi.fn(async (url: string | null) => {
        if (url && url.startsWith("http://")) throw new Error("plain http is allowed only for a coordinator on this machine");
        return { url, source: "settings" as const, settingsUrl: url, note: null };
      }),
    });
    const toast = vi.fn();
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={toast} />);
    await type(q(host, "fl-coordinator-input"), "http://pool.example.org");
    await click(q(host, "fl-coordinator-save"));
    expect(q(host, "fl-error")!.textContent).toContain("plain http");
    await type(q(host, "fl-coordinator-input"), "https://pool.example.org");
    await click(q(host, "fl-coordinator-save"));
    expect(fl.setCoordinator).toHaveBeenLastCalledWith("https://pool.example.org");
  });

  it("labels an env override as the operator's setting", async () => {
    const fl = domain({
      overview: vi.fn(async () =>
        overview({ config: { url: "https://env.example.org", source: "env", settingsUrl: null, note: "CITRATE_FL_COORDINATOR_URL overrides the setting" } }),
      ),
    });
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    expect(q(host, "fl-coordinator")!.textContent).toContain("https://env.example.org");
    expect(q(host, "fl-coordinator")!.textContent).toContain("CITRATE_FL_COORDINATOR_URL");
  });
});

describe("FlRoundsPanel — plan and start", () => {
  it("shows the four plain-words answers after planning", async () => {
    const { host } = await mount(<FlRoundsPanel fl={domain()} requestSig={vi.fn()} toast={vi.fn()} />);
    await click(q(host, "fl-plan"));
    const text = q(host, "fl-plan-explain")!.textContent!;
    for (const k of ["Data", "Compute", "Reward", "Privacy"]) expect(text).toContain(k);
    expect(text).toContain("nothing is paid");
  });

  it("disables Start and lists the blockers when the plan cannot start", async () => {
    const fl = domain({ plan: vi.fn(async () => livePlan({ canStart: false, blockers: ["No training coordinator is configured."] })) });
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    await click(q(host, "fl-plan"));
    expect(q<HTMLButtonElement>(host, "fl-start")!.disabled).toBe(true);
    expect(q(host, "fl-blockers")!.textContent).toContain("No training coordinator is configured.");
  });

  it("asks the member (HIC-1) and starts only that plan hash after approval, then says no training ran", async () => {
    const fl = domain();
    const requestSig = vi.fn(async () => "approved");
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={requestSig} toast={vi.fn()} />);
    await click(q(host, "fl-plan"));
    await click(q(host, "fl-start"));
    expect(requestSig).toHaveBeenCalledTimes(1);
    expect(fl.start).toHaveBeenCalledWith(PLAN_HASH);
    expect(q(host, "fl-receipt")!.textContent).toContain("no training has started");
  });

  it("does not start when the member declines", async () => {
    const fl = domain();
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn(async () => "declined")} toast={vi.fn()} />);
    await click(q(host, "fl-plan"));
    await click(q(host, "fl-start"));
    expect(fl.start).not.toHaveBeenCalled();
  });
});

describe("FlRoundsPanel — adapter eval gate", () => {
  async function fillGate(host: HTMLElement) {
    await type(q(host, "fl-gate-adapter"), "/x/a.gguf");
    await type(q(host, "fl-gate-sha"), "c".repeat(64));
    await type(q(host, "fl-gate-base-tools"), "/x/base.json");
    await type(q(host, "fl-gate-cand-tools"), "/x/cand.json");
    await click(q(host, "fl-gate-run"));
  }

  it("runs the gate with the given files and offers Load only on ACCEPT", async () => {
    const fl = domain();
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    await fillGate(host);
    expect(fl.gateAdapter).toHaveBeenCalledWith({
      adapterPath: "/x/a.gguf",
      expectedSha256: "c".repeat(64),
      baseToolsPath: "/x/base.json",
      candidateToolsPath: "/x/cand.json",
    });
    expect(q(host, "fl-gate-verdict")!.textContent).toMatch(/passed/i);
    await click(q(host, "fl-gate-load"));
    expect(fl.loadAdapter).toHaveBeenCalledWith("c".repeat(64));
  });

  it("shows the reasons and no Load button when the gate rejects", async () => {
    const fl = domain({ gateAdapter: vi.fn(async () => gateRec("REJECT")) });
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    await fillGate(host);
    expect(q(host, "fl-gate-verdict")!.textContent).toMatch(/rejected/i);
    expect(q(host, "fl-gate-verdict")!.textContent).toContain("did not improve");
    expect(q(host, "fl-gate-load")).toBeNull();
  });

  it("shows the loaded adapter and unloads it on request", async () => {
    const fl = domain({ overview: vi.fn(async () => overview({ activeAdapter: "/data/adapters/x.gguf" })) });
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    expect(q(host, "fl-active-adapter")!.textContent).toContain("/data/adapters/x.gguf");
    await click(q(host, "fl-unload"));
    expect(fl.unloadAdapter).toHaveBeenCalled();
  });
});

describe("FlRoundsPanel — web preview", () => {
  it("renders an honest message when core is not reachable", async () => {
    const fl = domain({ overview: vi.fn(async () => Promise.reject(new Error("federated rounds need the desktop app"))) });
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    expect(q(host, "fl-error")!.textContent).toContain("need the desktop app");
  });
  it("says the eval --model must be the served base model's file name (load compares it)", async () => {
    // Review fix: the gate records the scorecards' model as the adapter's base and load refuses unless
    // it matches the served GGUF file name, so a short alias passed as --model would make every load fail.
    const { host } = await mount(<FlRoundsPanel fl={domain()} requestSig={vi.fn()} toast={vi.fn()} />);
    expect(host.textContent).toContain("--model set to the base model's file name");
  });
});

// ---------------------------------------------------------------------------
// HUP-S9.4 rest (fan-out 6): named rounds and consent, round results, re-apply after a restart.
// ---------------------------------------------------------------------------
const ROUND = "0x29a0bbad3829ef8c62a4ba4b8c6bb61f0e893db107b033a80e55171f159a3dfe";

describe("FlRoundsPanel — a named round and its consent", () => {
  it("plans with the round id the member typed, and without one when the field is empty", async () => {
    const fl = domain();
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    await click(q(host, "fl-plan"));
    expect(fl.plan).toHaveBeenLastCalledWith(undefined);
    await type(q(host, "fl-round-id"), " " + ROUND + " ");
    await click(q(host, "fl-plan"));
    expect(fl.plan).toHaveBeenLastCalledWith(expect.objectContaining({ roundId: ROUND, requires: "federated" }));
  });

  it("lists the rounds this device consented to, with the worker's file, and withdraws one", async () => {
    const fl = domain({
      overview: vi.fn(async () => overview({ consentedRounds: [ROUND] })),
      revokeConsent: vi.fn(async () => []),
    });
    const toast = vi.fn();
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={toast} />);
    const box = q(host, "fl-consents")!;
    expect(box.textContent).toContain(ROUND);
    expect(box.textContent).toContain("/data/fl_consent.json");
    expect(box.textContent).toContain("CITRATE_FL_CONSENT_FILE");
    await click(q(host, "fl-consent-revoke-0"));
    expect(fl.revokeConsent).toHaveBeenCalledWith(ROUND);
    expect(toast).toHaveBeenCalled();
  });

  it("shows core's error when the consent file cannot be read", async () => {
    const fl = domain({ overview: vi.fn(async () => overview({ consentError: "fl_consent.json is unreadable" })) });
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    expect(q(host, "fl-consents")!.textContent).toContain("unreadable");
  });
});

describe("FlRoundsPanel — round results and re-apply", () => {
  it("sends the round result path, and lets the round supply the expected hash", async () => {
    const fl = domain();
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    await type(q(host, "fl-gate-adapter"), "/x/merged.gguf");
    await type(q(host, "fl-gate-round"), "/x/round.json");
    await type(q(host, "fl-gate-base-tools"), "/x/base.json");
    await type(q(host, "fl-gate-cand-tools"), "/x/cand.json");
    await click(q(host, "fl-gate-run"));
    expect(fl.gateAdapter).toHaveBeenCalledWith({
      adapterPath: "/x/merged.gguf",
      expectedSha256: "",
      baseToolsPath: "/x/base.json",
      candidateToolsPath: "/x/cand.json",
      roundResultPath: "/x/round.json",
    });
  });

  it("shows the round a gated adapter came from", async () => {
    const rec = gateRec("ACCEPT");
    rec.round = { roundId: ROUND, recordDigest: "0x" + "6".repeat(64), adapterSha256: "c".repeat(64), chainId: 1337, ledger: "0x" + "7".repeat(40), participants: 3 };
    const fl = domain({ gateAdapter: vi.fn(async () => rec) });
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    await type(q(host, "fl-gate-adapter"), "/x/merged.gguf");
    await type(q(host, "fl-gate-round"), "/x/round.json");
    await click(q(host, "fl-gate-run"));
    expect(q(host, "fl-gate-verdict")!.textContent).toContain(ROUND);
  });

  it("says a loaded adapter is put back after a restart, and why it was not when that failed", async () => {
    const fl = domain({
      overview: vi.fn(async () =>
        overview({
          activeAdapter: "/data/adapters/x.gguf",
          rememberedAdapter: { sha256: "c".repeat(64), baseModel: "m.gguf" },
          restoreError: null,
        }),
      ),
    });
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    expect(q(host, "fl-remembered")!.textContent).toMatch(/put back when the app starts the local model on m\.gguf/);
    const failed = domain({
      overview: vi.fn(async () =>
        overview({ rememberedAdapter: { sha256: "c".repeat(64), baseModel: "m.gguf" }, restoreError: "The adapter you loaded earlier was not put back: the adapter file changed since the eval gate; run the gate again" }),
      ),
    });
    const second = await mount(<FlRoundsPanel fl={failed} requestSig={vi.fn()} toast={vi.fn()} />);
    expect(q(second.host, "fl-remembered")!.textContent).toContain("was not put back");
  });
});
