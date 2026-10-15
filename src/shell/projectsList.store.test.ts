// HGW-W3 add-on: when the store keeps a project Hermes created, or the member allows the workspace
// card from the chat, every open Projects list re-reads (so the Agent surface never shows a stale
// list).
import { describe, it, expect, vi, afterEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import { onProjectsChanged } from "../agent/projects/projects";

afterEach(() => {
  vi.restoreAllMocks();
});

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("HGW-W3 store: the Projects list follows the chat", () => {
  it("a kept project re-reads the list", async () => {
    vi.spyOn(bridge.agentHarness, "projectsRecord").mockResolvedValue(["/Users/m/Citrate/Projects/demo"]);
    const seen = vi.fn();
    const off = onProjectsChanged(seen);
    expect(store.projectActivity({ kind: "project_created", path: "/Users/m/Citrate/Projects/demo" })).toBe(true);
    await flush();
    off();
    expect(seen).toHaveBeenCalledTimes(1);
  });

  it("a project core refused to keep does not", async () => {
    vi.spyOn(bridge.agentHarness, "projectsRecord").mockRejectedValue(new Error("no marker"));
    vi.spyOn(store, "toast").mockImplementation(() => undefined);
    const seen = vi.fn();
    const off = onProjectsChanged(seen);
    store.projectActivity({ kind: "project_created", path: "/x/bad" });
    await flush();
    off();
    expect(seen).not.toHaveBeenCalled();
  });

  it("allowing the workspace from the chat re-reads the list", async () => {
    vi.spyOn(bridge.agentHarness, "projectsWorkspace").mockResolvedValue({ root: "/Users/m/Citrate/Projects", exists: false, granted: false });
    vi.spyOn(bridge.agentHarness, "projectsWorkspaceGrant").mockResolvedValue({ root: "/Users/m/Citrate/Projects", exists: true, granted: true });
    vi.spyOn(store, "requestSig").mockResolvedValue("approved");
    vi.spyOn(store, "toast").mockImplementation(() => undefined);
    (store as unknown as { projectsWorkspaceOffered: boolean }).projectsWorkspaceOffered = false;
    const seen = vi.fn();
    const off = onProjectsChanged(seen);
    await store.offerProjectsWorkspace();
    off();
    expect(seen).toHaveBeenCalledTimes(1);
  });
});
