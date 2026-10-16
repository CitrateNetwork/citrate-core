// HUP-S3.1 — first-run knowledge-corpus import: bridge contract.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { bridge } from "../index";
import { deadlineFor } from "../tauri/invoke";

const { invokeMock, listenMock, unlistenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
  unlistenMock: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
import { tauriImportKnowledge, KNOWLEDGE_PROGRESS_EVENT } from "../tauri/knowledge";

describe("HUP-S3.1 knowledge import — bridge", () => {
  it("is part of the memory domain", () => {
    expect(typeof bridge.memory.importKnowledge).toBe("function");
  });

  it("sim is an honest skip (no corpus in the web preview), never a fabricated import", async () => {
    if (bridge.mode === "sim") {
      const r = await bridge.memory.importKnowledge();
      expect(r.state).toBe("skipped");
      expect(r.skipped).toBe("no-bundle");
      expect(r.nodesAdded).toBe(0);
    }
  });

  it("is never pre-empted by the invoke backstop (a first-run import can take minutes)", () => {
    expect(deadlineFor("memory_import_knowledge")).toBe(Infinity);
  });
});

describe("HUP-S3.1 knowledge import — tauri impl", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
    unlistenMock.mockReset();
  });

  it("subscribes to progress, invokes memory_import_knowledge, forwards lines, then unlistens", async () => {
    let handler: ((ev: { payload: unknown }) => void) | undefined;
    listenMock.mockImplementation(async (name: string, h: (ev: { payload: unknown }) => void) => {
      expect(name).toBe(KNOWLEDGE_PROGRESS_EVENT);
      handler = h;
      return unlistenMock;
    });
    invokeMock.mockImplementation(async () => {
      handler?.({ payload: { event: "progress", tenant: "citrate-docs", done: 128, total: 1688 } });
      handler?.({ payload: { event: "tenant_done", tenant: "citrate-docs" } });
      return { state: "imported", nodesAdded: 1688, edgesAdded: 1577, tenantsImported: ["citrate-docs"], tenantsSkipped: [] };
    });
    const seen: unknown[] = [];
    const r = await tauriImportKnowledge((line) => seen.push(line));
    expect(invokeMock).toHaveBeenCalledWith("memory_import_knowledge");
    expect(r.state).toBe("imported");
    expect(seen).toHaveLength(2);
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("unlistens even when the import rejects", async () => {
    listenMock.mockResolvedValue(unlistenMock);
    invokeMock.mockRejectedValue(new Error("boom"));
    await expect(tauriImportKnowledge(() => {})).rejects.toThrow("boom");
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });
});
