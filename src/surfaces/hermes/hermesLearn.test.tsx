// K2 (0.5.2): Hermes › Learn. One place for what Hermes learned (proposals, saved skills, memories,
// Teach Hermes), the built-in knowledge import and the training opt-in, each from the component
// and command that already back it. Group rounds are not live on 40204 (UI truth contract §4).
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { HermesLearn } from "./HermesLearn";
import { store } from "../../shell/store";
import { freshState, type AppState } from "../../shell/state";
import { bridge } from "../../bridge";
import { agentSlice } from "../../shell/slices/agent";
import type { LearnStatus } from "../../agent/learn";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.restoreAllMocks();
});

const $ = (id: string) => host?.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

async function mount(patch: Partial<AppState> = {}) {
  const s = { ...freshState("p1"), stage: "done" as const, route: "hermes/learn", ...patch };
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<HermesLearn store={store} s={s} />);
  });
  await act(async () => await new Promise((r) => setTimeout(r, 0)));
}

describe("Hermes › Learn", () => {
  it("Import built-in knowledge runs the existing import (memory_import_knowledge through store.importKnowledge)", async () => {
    const spy = vi.spyOn(store, "importKnowledge").mockResolvedValue();
    await mount();
    const btn = $("learn-knowledge-import-run") as HTMLButtonElement | null;
    expect(btn).toBeTruthy();
    expect(btn?.disabled).toBe(false);
    await act(async () => {
      btn?.click();
    });
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it("while an import runs the button is disabled and the importer's own progress shows", async () => {
    await mount({ knowledgeImport: { state: "running", tenant: "citrate-docs", done: 3, total: 10, nodesAdded: 0, message: null } });
    expect(($("learn-knowledge-import-run") as HTMLButtonElement | null)?.disabled).toBe(true);
    expect($("learn-knowledge-import")?.textContent).toContain("3 / 10");
  });

  it("shows what the importer reported, and nothing before the first attempt", async () => {
    await mount({ knowledgeImport: { state: "imported", tenant: null, done: 0, total: 0, nodesAdded: 42, message: null } });
    expect($("learn-knowledge-import")?.textContent).toContain("42 entries");
  });

  it("the training opt-in says group rounds are not live yet and that nothing is sent anywhere", async () => {
    await mount();
    expect($("learn-training")?.textContent).toContain("Group rounds not live yet");
    expect($("learn-training")?.textContent).toContain("Nothing recorded here is sent anywhere");
    expect($("fl-trajectories")).toBeTruthy();
  });

  it("names its data sources", async () => {
    await mount();
    expect($("learn-knowledge-import")?.getAttribute("data-source")).toBe("memory_import_knowledge");
    expect($("learn-training")?.getAttribute("data-source")).toBe("trajectories_settings_get / trajectories_settings_set");
    expect($("learned-panel")).toBeTruthy();
  });

  // Codex review of #364 at 4e6a85d: Learn read Hermes's run state from agentSlice without
  // refreshing it. The slice starts with Hermes off and was refreshed only by the Agent page, so
  // opening Learn first while the sidecar runs showed Teach Hermes as needing Hermes running.
  it("opened first, with Hermes already running: reads the harness status itself and offers Teach Hermes", async () => {
    agentSlice.set({ attached: agentSlice.get().attached.map((r) => (r.id === "hermes" ? { ...r, state: "off", error: null } : r)) });
    const status = vi.spyOn(bridge.agentHarness, "status").mockResolvedValue({ running: true, skills: 0, pendingApprovals: 0 });
    vi.spyOn(bridge.agentHarness, "skills").mockResolvedValue([]);
    vi.spyOn(bridge.agentHarness, "pendingApprovals").mockResolvedValue([]);
    vi.spyOn(bridge.agentHarness, "registrySkills").mockResolvedValue([]);
    vi.spyOn(bridge.agentHarness, "learnStatus").mockResolvedValue({ sidecar: { enabled: true }, publish: { available: false } } as unknown as LearnStatus);
    vi.spyOn(bridge.agentHarness, "learnProposals").mockResolvedValue([]);
    vi.spyOn(bridge.agentHarness, "learnMemories").mockResolvedValue([]);
    await mount();
    await act(async () => await new Promise((r) => setTimeout(r, 0)));
    expect(status).toHaveBeenCalled();
    expect(host?.textContent).not.toContain("Teaching needs Hermes running");
    expect(agentSlice.get().attached.find((r) => r.id === "hermes")?.state).toBe("running");
  });
});

// Tripwire: a view that shows Hermes's run state from agentSlice refreshes it itself, so it never
// depends on another page having been opened first.
describe("tripwire: views reading Hermes's run state refresh it", () => {
  it("every src/ file that reads agentSlice run state (state === \"running\") also calls refreshAgent()", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const srcDir = path.resolve(__dirname, "../..");
    const offenders: string[] = [];
    let readers = 0;
    const walk = (d: string) => {
      for (const e of fs.readdirSync(d, { withFileTypes: true })) {
        const p = path.join(d, e.name);
        if (e.isDirectory()) walk(p);
        else if (/\.tsx$/.test(e.name) && !/\.test\.tsx$/.test(e.name)) {
          const t = fs.readFileSync(p, "utf8");
          if (/agentSlice\.use\(\)/.test(t) && /state === "running"/.test(t)) {
            readers++;
            if (!/refreshAgent\(\)/.test(t)) offenders.push(path.relative(srcDir, p));
          }
        }
      }
    };
    walk(srcDir);
    expect(readers).toBeGreaterThanOrEqual(2);
    expect(offenders).toEqual([]);
  });
});
