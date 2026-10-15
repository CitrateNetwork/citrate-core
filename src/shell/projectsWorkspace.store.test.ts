// HGW-W3 (core #289): the store keeps each project Hermes creates (core checks it), and offers the
// workspace card ("Let Hermes create and work in ~/Citrate/Projects") when project_create finds the
// projects folder not set up. Only an explicit Approve creates and shares the folder.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";

const calls: Array<[string, Record<string, unknown>]> = [];
let ready = false;
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri" }));
vi.mock("../agent/projects/projects", async (orig) => {
  const real = await orig<typeof import("../agent/projects/projects")>();
  return {
    ...real,
    desktopProjectsIo: async () => ({
      mode: "tauri" as const,
      pickFolder: async () => null,
      invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
        calls.push([cmd, args]);
        if (cmd === "hermes_projects_record_created" && String(args.path).includes("bad")) throw new Error("/x/bad has no project marker from Hermes");
        return { root: "/Users/m/Citrate/Projects", rootExists: ready, workspaceReady: ready, projects: [], loadError: null } as T;
      },
    }),
  };
});

import { store } from "./store";

beforeEach(() => {
  calls.length = 0;
  ready = false;
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe("HGW-W3 store: projects", () => {
  it("keeps a created project through core", async () => {
    await store.recordHermesProject({ name: "demo", path: "/Users/m/Citrate/Projects/demo" });
    expect(calls).toContainEqual(["hermes_projects_record_created", { path: "/Users/m/Citrate/Projects/demo" }]);
  });

  it("says so when core refuses to keep it", async () => {
    const toast = vi.spyOn(store, "toast");
    await store.recordHermesProject({ name: "bad", path: "/x/bad" });
    expect(toast.mock.calls[0][0]).toContain("could not be added to your projects");
  });

  it("the workspace card creates and shares the folder only on Approve", async () => {
    const sig = vi.spyOn(store, "requestSig").mockResolvedValueOnce("declined").mockResolvedValueOnce("approved");
    await store.askProjectsWorkspace();
    expect(calls.map((c) => c[0])).not.toContain("hermes_projects_workspace_enable");
    await store.askProjectsWorkspace();
    expect(calls.map((c) => c[0])).toContain("hermes_projects_workspace_enable");
    const spec = sig.mock.calls[0][0];
    expect(spec.title).toBe("Let Hermes create and work in ~/Citrate/Projects");
    expect(spec.hic?.reason).toMatch(/projects folder/);
    expect(spec.chainless).toBe(true);
    expect(spec.card?.kind).toBe("fields");
  });

  it("asks nothing when the workspace is already set up", async () => {
    ready = true;
    const sig = vi.spyOn(store, "requestSig");
    await store.askProjectsWorkspace();
    expect(sig).not.toHaveBeenCalled();
  });
});
