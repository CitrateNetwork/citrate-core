// HGW-W3 (core #289): the Projects list on the Agent surface. The workspace card shows until the
// projects folder is created and shared (then it is remembered); projects show whether they still
// go to Hermes; Forget keeps the files; marking a folder goes through the native picker; the web
// preview says it needs the desktop app.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ProjectsPanel } from "./ProjectsPanel";
import type { ProjectsIo, ProjectsView } from "./projects";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ROOT = "/Users/m/Citrate/Projects";
function view(over: Partial<ProjectsView> = {}): ProjectsView {
  return { root: ROOT, rootExists: false, workspaceReady: false, projects: [], loadError: null, ...over };
}
const demo = { path: ROOT + "/demo", name: "demo", origin: "hermes" as const, addedAt: 1, exists: true, shared: true };
const site = { path: "/Users/m/work/site", name: "site", origin: "member" as const, addedAt: 2, exists: false, shared: false };

type Calls = Array<[string, Record<string, unknown>]>;
function fakeIo(answers: Record<string, unknown>, calls: Calls, mode: "tauri" | "sim" = "tauri", picked: string | null = null): () => Promise<ProjectsIo> {
  return async () => ({
    mode,
    pickFolder: async () => picked,
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      calls.push([cmd, args]);
      const a = answers[cmd];
      if (a instanceof Error) throw a;
      return a as T;
    },
  });
}

async function mount(io: () => Promise<ProjectsIo>): Promise<{ el: HTMLDivElement; root: Root }> {
  const el = document.createElement("div");
  document.body.appendChild(el);
  const root = createRoot(el);
  await act(async () => {
    root.render(<ProjectsPanel io={io} />);
  });
  await act(async () => {
    await Promise.resolve();
  });
  return { el, root };
}

const q = (el: HTMLElement, id: string) => el.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

describe("Projects panel (HGW-W3)", () => {
  it("offers the workspace card until the projects folder is shared, then remembers it", async () => {
    const calls: Calls = [];
    const io = fakeIo({ hermes_projects_view: view(), hermes_projects_workspace_enable: view({ rootExists: true, workspaceReady: true }) }, calls);
    const { el, root } = await mount(io);
    expect(q(el, "projects-workspace-card")?.textContent).toContain("Let Hermes create and work in ~/Citrate/Projects");
    await act(async () => {
      q(el, "projects-workspace-allow")?.click();
    });
    expect(calls.map((c) => c[0])).toContain("hermes_projects_workspace_enable");
    expect(q(el, "projects-workspace-card")).toBeNull();
    expect(q(el, "projects-root")?.textContent).toContain(ROOT);
    act(() => root.unmount());
  });

  it("lists projects with whether they still go to Hermes, and Forget keeps the files", async () => {
    const calls: Calls = [];
    const io = fakeIo(
      {
        hermes_projects_view: view({ rootExists: true, workspaceReady: true, projects: [demo, site] }),
        hermes_projects_forget: view({ rootExists: true, workspaceReady: true, projects: [demo] }),
      },
      calls,
    );
    const { el, root } = await mount(io);
    const rows = el.querySelectorAll('[data-testid="project-row"]');
    expect(rows.length).toBe(2);
    expect(rows[0].textContent).toContain("created by Hermes");
    expect(rows[1].textContent).toContain("folder missing");
    await act(async () => {
      (rows[1].querySelector('[data-testid="project-forget"]') as HTMLElement).click();
    });
    expect(calls).toContainEqual(["hermes_projects_forget", { path: site.path }]);
    expect(el.querySelectorAll('[data-testid="project-row"]').length).toBe(1);
    act(() => root.unmount());
  });

  it("marks a folder picked in the native dialog, and shows a refusal", async () => {
    const calls: Calls = [];
    const io = fakeIo(
      {
        hermes_projects_view: view({ rootExists: true, workspaceReady: true }),
        hermes_projects_mark: new Error("/Users/m/work/site is not shared with Hermes for reading and writing; share it in Settings > Folder access first"),
      },
      calls,
      "tauri",
      "/Users/m/work/site",
    );
    const { el, root } = await mount(io);
    await act(async () => {
      q(el, "projects-mark")?.click();
    });
    expect(calls).toContainEqual(["hermes_projects_mark", { path: "/Users/m/work/site" }]);
    expect(q(el, "projects-error")?.textContent).toContain("not shared with Hermes");
    act(() => root.unmount());
  });

  it("a cancelled picker sends nothing", async () => {
    const calls: Calls = [];
    const { el, root } = await mount(fakeIo({ hermes_projects_view: view({ workspaceReady: true, rootExists: true }) }, calls, "tauri", null));
    await act(async () => {
      q(el, "projects-mark")?.click();
    });
    expect(calls.map((c) => c[0])).toEqual(["hermes_projects_view"]);
    act(() => root.unmount());
  });

  it("says when the projects list cannot be read", async () => {
    const { el, root } = await mount(fakeIo({ hermes_projects_view: view({ loadError: "the projects list is not valid; no project is sent to Hermes" }) }, []));
    expect(q(el, "projects-load-error")?.textContent).toContain("no project is sent to Hermes");
    act(() => root.unmount());
  });

  it("the web preview says projects need the desktop app", async () => {
    const calls: Calls = [];
    const { el, root } = await mount(fakeIo({}, calls, "sim"));
    expect(q(el, "projects-desktop-only")?.textContent).toContain("desktop app");
    expect(calls).toEqual([]);
    act(() => root.unmount());
  });

  it("copy has no em-dashes", async () => {
    const { el, root } = await mount(fakeIo({ hermes_projects_view: view({ projects: [demo, site] }) }, []));
    expect(el.textContent ?? "").not.toMatch(/—/);
    act(() => root.unmount());
  });
});

describe("Projects panel refresh (HGW-W3)", () => {
  it("re-reads when core kept a project from the chat", async () => {
    const { notifyProjectsChanged } = await import("./projects");
    const calls: Calls = [];
    const { el, root } = await mount(fakeIo({ hermes_projects_view: view({ workspaceReady: true, rootExists: true }) }, calls));
    const before = calls.filter((c) => c[0] === "hermes_projects_view").length;
    await act(async () => {
      notifyProjectsChanged();
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(calls.filter((c) => c[0] === "hermes_projects_view").length).toBe(before + 1);
    act(() => root.unmount());
    expect(el).toBeTruthy();
  });
});
