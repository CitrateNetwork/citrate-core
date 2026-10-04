// HUP-S9.4 — the federated-rounds panel on the Train surface. Written red-first (the component did
// not exist). The domain is a typed in-test double of FlRoundsDomain; production uses core.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { FlAdapterGateRecord, FlOverview, FlRoundProvenance, FlRoundsDomain } from "../bridge/domains";
import { FlRoundsPanel } from "./FlRoundsPanel";
import { livePlan, PLAN_HASH } from "./fixtures/plan";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function overview(over: Partial<FlOverview> = {}): FlOverview {
  return {
    config: { url: null, source: "none", settingsUrl: null, note: null },
    starts: [],
    gates: [],
    activeAdapter: null,
    remembered: null,
    rounds: [],
    eval: null,
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
    importRound: vi.fn(async () => provenance()),
    fetchRound: vi.fn(async () => provenance()),
    fetchAdapter: vi.fn(async (_u: string, sha: string) => "/data/adapters/incoming/" + sha + ".gguf"),
    evalBegin: vi.fn(async (_p: string, sha: string) => ({ sessionId: "s1", adapterSha256: sha, model: "m", startedAtMs: 1, baseCalls: 0, candidateCalls: 0 })),
    evalComplete: vi.fn(async () => JSON.stringify({ role: "assistant", content: "ok", tool_calls: [] })),
    evalFinish: vi.fn(async () => gateRec("ACCEPT")),
    evalEnd: vi.fn(async () => {}),
    ...over,
  };
}

function provenance(): FlRoundProvenance {
  return {
    roundId: "29".repeat(32),
    ordinal: 0,
    chainId: 1337,
    ledger: "76".repeat(20),
    clusterId: "09".repeat(32),
    baseModelSha256: "a5".repeat(32),
    startAdapterSha256: "55".repeat(32),
    adapterSha256: "ae".repeat(32),
    recordDigest: "69".repeat(32),
    participants: 3,
    minParticipants: 3,
    excluded: 0,
    stateCounts: [50, 25, 0, 25],
    adapterPath: "/data/adapters/incoming/" + "ae".repeat(32) + ".gguf",
    checkedAtMs: 1,
    chainRecord: "The round's on-chain record is not checked by this build.",
    notes: ["This round was recorded on chain 1337, not the Citrate network (40204): a local or test round."],
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

describe("FlRoundsPanel — a round's adapter (n5)", () => {
  it("checks two local files through core and fills the gate with the round's adapter", async () => {
    const fl = domain();
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    await type(q(host, "fl-round-bundle"), "/x/bundle.json");
    await type(q(host, "fl-round-adapter"), "/x/merged.gguf");
    await click(q(host, "fl-round-get"));
    expect(fl.importRound).toHaveBeenCalledWith("/x/bundle.json", "/x/merged.gguf");
    expect(fl.fetchRound).not.toHaveBeenCalled();
    const sum = q(host, "fl-round-summary")!.textContent ?? "";
    expect(sum).toContain("3 devices took part");
    expect(sum).toContain("not the Citrate network");
    expect(sum).toContain("not checked");
    expect((q<HTMLInputElement>(host, "fl-gate-sha"))!.value).toBe("ae".repeat(32));
    expect((q<HTMLInputElement>(host, "fl-gate-adapter"))!.value).toContain("incoming");
  });

  it("downloads when both are links, and refuses a mix of a path and a link", async () => {
    const fl = domain();
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    await type(q(host, "fl-round-bundle"), "https://mirror.example.org/round.json");
    await type(q(host, "fl-round-adapter"), "/x/merged.gguf");
    await click(q(host, "fl-round-get"));
    expect(q(host, "fl-error")!.textContent).toContain("not one of each");
    expect(fl.fetchRound).not.toHaveBeenCalled();
    await type(q(host, "fl-round-adapter"), "https://mirror.example.org/merged.gguf");
    await click(q(host, "fl-round-get"));
    expect(fl.fetchRound).toHaveBeenCalledWith("https://mirror.example.org/round.json", "https://mirror.example.org/merged.gguf");
  });

  it("downloads an adapter by its published sha256 and fills the gate's path", async () => {
    const fl = domain();
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    await type(q(host, "fl-gate-sha"), "d".repeat(64));
    await type(q(host, "fl-adapter-url"), "https://mirror.example.org/a.gguf");
    await click(q(host, "fl-adapter-fetch"));
    expect(fl.fetchAdapter).toHaveBeenCalledWith("https://mirror.example.org/a.gguf", "d".repeat(64));
    expect((q<HTMLInputElement>(host, "fl-gate-adapter"))!.value).toBe("/data/adapters/incoming/" + "d".repeat(64) + ".gguf");
  });

  it("shows core's refusal of a round that does not match", async () => {
    const fl = domain({ importRound: vi.fn(async () => Promise.reject(new Error("this round trained an adapter for base model sha256 00, but the model this app serves has sha256 a5"))) });
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    await type(q(host, "fl-round-bundle"), "/x/bundle.json");
    await type(q(host, "fl-round-adapter"), "/x/merged.gguf");
    await click(q(host, "fl-round-get"));
    expect(q(host, "fl-error")!.textContent).toContain("trained an adapter for base model");
    expect(q(host, "fl-round-summary")).toBeNull();
  });
});

describe("FlRoundsPanel — the eval in the app (n5)", () => {
  it("runs both arms through core and shows core's verdict with Load on ACCEPT", async () => {
    const fl = domain();
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    await type(q(host, "fl-gate-adapter"), "/x/a.gguf");
    await type(q(host, "fl-gate-sha"), "c".repeat(64));
    await act(async () => {
      (q(host, "fl-eval-run") as HTMLElement).click();
    });
    await vi.waitFor(() => expect(fl.evalFinish).toHaveBeenCalled(), { timeout: 5000 });
    await act(async () => {});
    expect(fl.evalBegin).toHaveBeenCalledWith("/x/a.gguf", "c".repeat(64));
    // 88 items per arm: the shipped tool-call + injection sets.
    expect((fl.evalComplete as ReturnType<typeof vi.fn>).mock.calls.length).toBe(176);
    expect(q(host, "fl-gate-verdict")!.textContent).toMatch(/passed/i);
    expect(q(host, "fl-gate-load")).toBeTruthy();
  });

  it("is disabled until an adapter and its sha256 are given", async () => {
    const { host } = await mount(<FlRoundsPanel fl={domain()} requestSig={vi.fn()} toast={vi.fn()} />);
    expect((q<HTMLButtonElement>(host, "fl-eval-run"))!.disabled).toBe(true);
  });

  it("shows core's refusal and ends the run", async () => {
    const fl = domain({ evalBegin: vi.fn(async () => Promise.reject(new Error("start the local model first; the eval runs on it"))) });
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    await type(q(host, "fl-gate-adapter"), "/x/a.gguf");
    await type(q(host, "fl-gate-sha"), "c".repeat(64));
    await click(q(host, "fl-eval-run"));
    await vi.waitFor(() => expect(q(host, "fl-error")?.textContent ?? "").toContain("start the local model first"), { timeout: 5000 });
    expect(fl.evalComplete).not.toHaveBeenCalled();
  });

  it("says a remembered adapter comes back after a restart", async () => {
    const fl = domain({
      overview: vi.fn(async () => overview({ activeAdapter: "/data/adapters/x.gguf", remembered: { sha256: "c".repeat(64), baseModel: "m.gguf", loadedAtMs: 1 } })),
    });
    const { host } = await mount(<FlRoundsPanel fl={fl} requestSig={vi.fn()} toast={vi.fn()} />);
    expect(q(host, "fl-active-adapter")!.textContent).toContain("comes back after a restart");
  });
});
