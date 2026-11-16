// V1 (owner decision 2026-11-16): the per-project opt-ins for node, npm run and git commit.
//
// BDD:
//   Given Hermes lists opt-in rules, when the member picks a project and ticks one, then core saves
//     exactly that rule for that folder.
//   Given a saved project, when the member turns all off, then core removes it.
//   Given a Hermes build with no opt-in rules (or Hermes stopped), then nothing can be turned on and
//     the panel says why.
//   The web preview has no sidecar: the panel says so and calls nothing.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { ProjectOptInsPanel, OPT_INS_HINT, type OptInsView } from "./ProjectOptInsPanel";
import type { GrantsIo } from "./grants";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PROJ = "/Users/member/Citrate/Projects/demo";
const LIST = {
  enabled: true,
  rules: [
    { id: "ls", program: "ls" },
    { id: "node-script", program: "node", opt_in: true },
    { id: "npm-run", program: "npm", opt_in: true },
    { id: "git-commit", program: "git", opt_in: true },
  ],
};

type Calls = Array<[string, Record<string, unknown>]>;
function makeIo(opts: { list?: unknown; saved?: OptInsView; mode?: "tauri" | "sim"; pick?: string | null } = {}) {
  const calls: Calls = [];
  let saved: OptInsView = opts.saved ?? { projects: [], loadError: null };
  const io: GrantsIo = {
    mode: opts.mode ?? "tauri",
    pickFolder: vi.fn(async () => (opts.pick === undefined ? PROJ : opts.pick)),
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      calls.push([cmd, args]);
      if (cmd === "hermes_auto_approve_rules") return (opts.list === undefined ? LIST : opts.list) as T;
      if (cmd === "hermes_auto_approve_get") return saved as T;
      if (cmd === "hermes_auto_approve_set") {
        const rules = args.rules as string[];
        const root = args.root as string;
        const others = saved.projects.filter((p) => p.root !== root);
        saved = { projects: rules.length ? [...others, { root, rules, changedAtMs: 1 }] : others, loadError: null };
        return saved as T;
      }
      throw new Error("unexpected " + cmd);
    },
  };
  return { io, calls };
}

async function mount(io: GrantsIo) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<ProjectOptInsPanel io={async () => io} />);
  });
  await act(async () => {});
  return host;
}
async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await act(async () => {});
}
const button = (host: HTMLElement, text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent === text);
const box = (host: HTMLElement, label: string) =>
  [...host.querySelectorAll("label")].find((l) => l.textContent === label)?.querySelector("input") as HTMLInputElement | null;

describe("ProjectOptInsPanel (V1)", () => {
  it("picking a project and ticking one command saves exactly that rule for that folder", async () => {
    const { io, calls } = makeIo();
    const host = await mount(io);
    expect(host.textContent).toContain(OPT_INS_HINT);
    expect(host.textContent).toContain("No project has any of these turned on.");
    await click(button(host, "Choose a project folder"));
    expect(host.textContent).toContain(PROJ);
    // Picking alone saves nothing.
    expect(calls.filter(([c]) => c === "hermes_auto_approve_set")).toEqual([]);
    await click(box(host, "git commit"));
    expect(calls.filter(([c]) => c === "hermes_auto_approve_set")).toEqual([["hermes_auto_approve_set", { root: PROJ, rules: ["git-commit"] }]]);
    expect(box(host, "git commit")?.checked).toBe(true);
    expect(box(host, "node <file>")?.checked).toBe(false);
  });

  it("turning all off for a saved project removes it", async () => {
    const { io, calls } = makeIo({ saved: { projects: [{ root: PROJ, rules: ["npm-run", "node-script"], changedAtMs: 1 }], loadError: null } });
    const host = await mount(io);
    expect(box(host, "npm run build, test, lint or typecheck")?.checked).toBe(true);
    await click(button(host, "Turn all off for this project"));
    expect(calls.at(-1)).toEqual(["hermes_auto_approve_set", { root: PROJ, rules: [] }]);
    expect(host.textContent).not.toContain(PROJ);
  });

  it("a Hermes build without opt-in rules offers nothing to turn on, and says so", async () => {
    const { io } = makeIo({ list: { enabled: true, rules: [{ id: "ls" }, { id: "node-script" }] } });
    const host = await mount(io);
    expect(host.textContent).toContain("This Hermes build has no commands to turn on per project.");
    expect(button(host, "Choose a project folder")).toBeUndefined();
  });

  it("with Hermes stopped, nothing can be turned on and the panel says why", async () => {
    const { io } = makeIo({ list: null });
    const host = await mount(io);
    expect(host.textContent).toContain("Start Hermes to see which commands");
    expect(button(host, "Choose a project folder")).toBeUndefined();
  });

  it("the web preview has no Hermes to configure and calls nothing", async () => {
    const { io, calls } = makeIo({ mode: "sim" });
    const host = await mount(io);
    expect(host.textContent).toContain("desktop app");
    expect(calls).toEqual([]);
  });

  it("the copy has no em dashes and says the commands ask by default", () => {
    expect(OPT_INS_HINT).not.toMatch(/—/);
    expect(OPT_INS_HINT).toMatch(/ask you first by default/);
  });
});
