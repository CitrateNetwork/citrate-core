// Deleting a downloaded model from the Models screen: core decides what is deletable (in use,
// shipped with the app, downloading), the row shows Delete enabled or disabled with the reason,
// a confirmed delete re-reads the lists and the machine's free space, and a refusal is shown.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { bridge } from "../../bridge";
import { modelsSlice, deleteControl, deleteLocalModel, refreshLocalModels, type ModelsState } from "./models";
import { tierSlice } from "./tier";
import { sampleReport } from "./tierTestReport";
import { Models } from "../../surfaces/Models";
import { freshState } from "../state";
import type { Store } from "../store";
import type { ModelDeleteState, ModelDescriptor } from "../../bridge/domains";

const model = (file: string, sizeBytes = 2_000_000_000): ModelDescriptor => ({
  id: `local:${file}`,
  source: "bundled",
  repo: "",
  file,
  sizeBytes,
  sha256: "",
  kind: "gguf",
});
const ok = (file: string): ModelDeleteState => ({ file, sizeBytes: 1, deletable: true, reason: null });
const no = (file: string, reason: string): ModelDeleteState => ({ file, sizeBytes: 1, deletable: false, reason });

const reset = () =>
  modelsSlice.set({
    local: [],
    partials: [],
    activeId: null,
    downloadingId: null,
    selectingId: null,
    deleteStates: {},
    deletingFile: null,
    error: null,
  });

beforeEach(reset);
afterEach(() => {
  vi.restoreAllMocks();
  reset();
  tierSlice.set({ report: null, loaded: false });
});

describe("Delete control state", () => {
  const base = (): ModelsState => modelsSlice.get();

  it("is hidden when core said nothing about the model (web preview, failed read)", () => {
    expect(deleteControl(base(), "a.gguf")).toEqual({ show: false, disabled: true, reason: null });
  });

  it("is enabled when core says the model can be deleted", () => {
    const st = { ...base(), deleteStates: { "a.gguf": ok("a.gguf") } };
    expect(deleteControl(st, "a.gguf")).toEqual({ show: true, disabled: false, reason: null });
  });

  it("is disabled with core's reason when the model is in use or shipped with the app", () => {
    const st = {
      ...base(),
      deleteStates: {
        "busy.gguf": no("busy.gguf", "in use: the local model server is running on it for chat and Hermes"),
        "gemma.gguf": no("gemma.gguf", "this model ships with Citrate Core and comes back on the next start, so it cannot be deleted"),
      },
    };
    expect(deleteControl(st, "busy.gguf")).toMatchObject({ show: true, disabled: true });
    expect(deleteControl(st, "busy.gguf").reason).toMatch(/in use/);
    expect(deleteControl(st, "gemma.gguf").reason).toMatch(/ships with Citrate Core/);
  });

  it("waits while another delete, a download or a model switch runs", () => {
    const states = { "a.gguf": ok("a.gguf") };
    expect(deleteControl({ ...base(), deleteStates: states, deletingFile: "b.gguf" }, "a.gguf").disabled).toBe(true);
    expect(deleteControl({ ...base(), deleteStates: states, downloadingId: "hf:x" }, "a.gguf").reason).toMatch(/download/);
    expect(deleteControl({ ...base(), deleteStates: states, selectingId: "local:b.gguf" }, "a.gguf").disabled).toBe(true);
  });
});

describe("Deleting a model", () => {
  it("refreshing the local list also reads what can be deleted", async () => {
    vi.spyOn(bridge.modelsCatalog, "local").mockResolvedValue([model("a.gguf")]);
    vi.spyOn(bridge.modelsCatalog, "partials").mockResolvedValue([]);
    vi.spyOn(bridge.modelsCatalog, "deleteStates").mockResolvedValue([ok("a.gguf")]);
    await refreshLocalModels();
    expect(modelsSlice.get().deleteStates["a.gguf"]?.deletable).toBe(true);
  });

  it("a confirmed delete calls core, drops the row, clears the active pick and re-reads free space", async () => {
    modelsSlice.set({ local: [model("a.gguf"), model("b.gguf")], activeId: "local:a.gguf", deleteStates: { "a.gguf": ok("a.gguf") } });
    const del = vi
      .spyOn(bridge.modelsCatalog, "deleteLocal")
      .mockResolvedValue({ file: "a.gguf", freedBytes: 2_000_000_123, selectionCleared: true });
    vi.spyOn(bridge.modelsCatalog, "local").mockResolvedValue([model("b.gguf")]);
    vi.spyOn(bridge.modelsCatalog, "partials").mockResolvedValue([]);
    vi.spyOn(bridge.modelsCatalog, "deleteStates").mockResolvedValue([ok("b.gguf")]);
    const tier = vi.spyOn(bridge.tier, "recommend").mockResolvedValue(sampleReport());
    const out = await deleteLocalModel("a.gguf");
    expect(del).toHaveBeenCalledWith("a.gguf");
    expect(out?.freedBytes).toBe(2_000_000_123);
    const st = modelsSlice.get();
    expect(st.local.map((m) => m.file)).toEqual(["b.gguf"]);
    expect(st.activeId).toBeNull();
    expect(st.deletingFile).toBeNull();
    expect(st.deleteStates["a.gguf"]).toBeUndefined();
    expect(tier).toHaveBeenCalled();
  });

  it("a refusal from core is shown and nothing is dropped", async () => {
    modelsSlice.set({ local: [model("a.gguf")], deleteStates: { "a.gguf": ok("a.gguf") } });
    vi.spyOn(bridge.modelsCatalog, "deleteLocal").mockRejectedValue(new Error("in use: the local model server is running on it"));
    vi.spyOn(bridge.modelsCatalog, "deleteStates").mockResolvedValue([no("a.gguf", "in use: the local model server is running on it")]);
    expect(await deleteLocalModel("a.gguf")).toBeNull();
    const st = modelsSlice.get();
    expect(st.error).toMatch(/in use/);
    expect(st.local).toHaveLength(1);
    expect(st.deleteStates["a.gguf"].deletable).toBe(false);
  });

  it("one delete at a time", async () => {
    modelsSlice.set({ deletingFile: "a.gguf" });
    const del = vi.spyOn(bridge.modelsCatalog, "deleteLocal");
    expect(await deleteLocalModel("b.gguf")).toBeNull();
    expect(del).not.toHaveBeenCalled();
  });

  it("the web preview never pretends to delete", async () => {
    if (bridge.mode === "sim") {
      expect(await bridge.modelsCatalog.deleteStates()).toEqual([]);
      await expect(bridge.modelsCatalog.deleteLocal("a.gguf")).rejects.toThrow(/desktop app/);
    }
  });
});

describe("Models screen Delete", () => {
  const render = () => renderToStaticMarkup(<Models store={{} as unknown as Store} s={freshState("p1")} />);

  it("shows Delete on a deletable downloaded model and none on a model core said nothing about", () => {
    modelsSlice.set({ local: [model("a.gguf"), model("b.gguf")], deleteStates: { "a.gguf": ok("a.gguf") } });
    const html = render();
    expect(html).toContain('aria-label="Delete a.gguf"');
    expect(html).not.toContain('aria-label="Delete b.gguf"');
  });

  it("an in-use model's Delete is disabled and carries the reason as its tooltip", () => {
    modelsSlice.set({ local: [model("busy.gguf")], deleteStates: { "busy.gguf": no("busy.gguf", "in use: the local model server is running on it") } });
    const html = render();
    expect(html).toContain('title="in use: the local model server is running on it"');
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*aria-label="Delete busy.gguf"/);
  });
});
