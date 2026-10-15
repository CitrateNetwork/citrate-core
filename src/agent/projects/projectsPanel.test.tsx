// HGW-W3 add-on (core #289): the Projects list on the Agent surface. The workspace card shows until
// the projects folder is created and shared (then it is remembered); projects show whether they
// still go to Hermes; Forget keeps the files; marking a folder goes through the native picker; the
// list re-reads when the chat keeps a project; the web preview says it needs the desktop app.
import { describe, it, expect } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ProjectsPanel } from "./ProjectsPanel";
import { notifyProjectsChanged, WORKSPACE_HINT, WORKSPACE_TITLE, type ProjectsIo, type ProjectsList } from "./projects";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ROOT = "/Users/m/Citrate/Projects";
function list(over: Partial<ProjectsList> = {}, granted = false): ProjectsList {
  return { workspace: { root: ROOT, exists: granted, granted }, projects: [], ...over };
}
const demo = { path: ROOT + "/demo", name: "demo", origin: "hermes", exists: true, shared: true };
const site = { path: "/Users/m/work/site", name: "site", origin: "member", exists: false, shared: false };
const unshared = { path: "/Users/m/work/app", name: "app", origin: "member", exists: true, shared: false };

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

describe("Projects panel (HGW-W3 add-on)", () => {
  it("offers the workspace card until the projects folder is shared, then remembers it", async () => {
    const calls: Calls = [];
    const answers: Record<string, unknown> = { hermes_projects_list: list(), hermes_projects_workspace_grant: { root: ROOT, exists: true, granted: true } };
    const { el, root } = await mount(fakeIo(answers, calls));
    expect(q(el, "projects-workspace-card")?.textContent).toContain(WORKSPACE_TITLE);
    expect(q(el, "projects-workspace-card")?.textContent).toContain(WORKSPACE_HINT);
    answers.hermes_projects_list = list({}, true);
    await act(async () => {
      q(el, "projects-workspace-allow")?.click();
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(calls.map((c) => c[0])).toContain("hermes_projects_workspace_grant");
    expect(q(el, "projects-workspace-card")).toBeNull();
    expect(q(el, "projects-root")?.textContent).toContain(ROOT);
    act(() => root.unmount());
  });

  it("lists projects with whether they still go to Hermes, and Forget keeps the files", async () => {
    const calls: Calls = [];
    const io = fakeIo(
      {
        hermes_projects_list: list({ projects: [demo, site, unshared] }, true),
        hermes_projects_forget: list({ projects: [demo, unshared] }, true),
      },
      calls,
    );
    const { el, root } = await mount(io);
    const rows = el.querySelectorAll('[data-testid="project-row"]');
    expect(rows.length).toBe(3);
    expect(rows[0].textContent).toContain("created by Hermes");
    expect(rows[1].textContent).toContain("folder missing");
    expect(rows[2].textContent).toContain("not shared with Hermes");
    expect(q(el, "projects-root")?.textContent).toContain(ROOT);
    await act(async () => {
      (rows[1].querySelector('[data-testid="project-forget"]') as HTMLElement).click();
    });
    expect(calls).toContainEqual(["hermes_projects_forget", { path: site.path }]);
    expect(el.querySelectorAll('[data-testid="project-row"]').length).toBe(2);
    act(() => root.unmount());
  });

  it("marks a folder picked in the native dialog, and shows a refusal", async () => {
    const calls: Calls = [];
    const io = fakeIo(
      {
        hermes_projects_list: list({}, true),
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
    const { el, root } = await mount(fakeIo({ hermes_projects_list: list({}, true) }, calls, "tauri", null));
    await act(async () => {
      q(el, "projects-mark")?.click();
    });
    expect(calls.map((c) => c[0])).toEqual(["hermes_projects_list"]);
    act(() => root.unmount());
  });

  it("re-reads when core kept a project from the chat", async () => {
    const calls: Calls = [];
    const { root } = await mount(fakeIo({ hermes_projects_list: list({}, true) }, calls));
    const before = calls.filter((c) => c[0] === "hermes_projects_list").length;
    await act(async () => {
      notifyProjectsChanged();
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(calls.filter((c) => c[0] === "hermes_projects_list").length).toBe(before + 1);
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
    const { el, root } = await mount(fakeIo({ hermes_projects_list: list({ projects: [demo, site, unshared] }) }, []));
    expect(el.textContent ?? "").not.toMatch(/—/);
    expect(WORKSPACE_TITLE + WORKSPACE_HINT).not.toMatch(/—/);
    act(() => root.unmount());
  });
});
