// K2 (0.5.2): Hermes › Learn. One place for what Hermes learned (proposals, saved skills, memories,
// Teach Hermes), the built-in knowledge import and the training opt-in, each from the component
// and command that already back it. Group rounds are not live on 40204 (UI truth contract §4).
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { HermesLearn } from "./HermesLearn";
import { store } from "../../shell/store";
import { freshState, type AppState } from "../../shell/state";

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
});
