// HUP-S10.3 — the Daemons and Widgets cards on the Hermes home: honest states in the web preview,
// visible budgets (spend always 0, tokens estimated, defaults pending owner sign-off), pause and
// remove, and the gallery adding a template through the real save path.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { DaemonsPanel, type DaemonActions } from "./DaemonsPanel";
import { daemonsSlice } from "./slice";
import { WidgetsPanel } from "../widgets/WidgetsPanel";
import { widgetsSlice, type WidgetsApi } from "../widgets/api";
import { WIDGET_TEMPLATES } from "../widgets/gallery";
import type { DaemonsView } from "./api";
import type { WidgetSources } from "../widgets/catalog";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

vi.mock("@tauri-apps/api/core", async (orig) => ({
  ...(await orig<typeof import("@tauri-apps/api/core")>()),
  convertFileSrc: (id: string, scheme: string) => `${scheme}://localhost/${id}`,
}));

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});
function render(el: React.ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => root?.render(el));
  return host;
}
const $ = (el: HTMLElement, id: string) => el.querySelector(`[data-testid="${id}"]`) as HTMLElement;
const $$ = (el: HTMLElement, id: string) => Array.from(el.querySelectorAll(`[data-testid="${id}"]`)) as HTMLElement[];
const flush = () => act(async () => await Promise.resolve());

const view: DaemonsView = {
  allPaused: false,
  daemons: [
    {
      id: "d0123456789abcdef",
      name: "Node digest",
      prompt: "Summarise my node.",
      schedule: "0 9 * * *",
      budget: { maxRunsPerDay: 4, maxTokensPerDay: 20_000, maxTokensPerRun: 6_000, maxSpendSalt: "0" },
      paused: false,
      status: "budget used up today",
      running: false,
      runsToday: 4,
      tokensToday: 20_100,
      skippedToday: 3,
      spendTodaySalt: "0",
      nextRunMs: null,
      lastRunMs: 1,
      lastOutcome: "over_budget",
      lastNote: "stopped: the run reached its token allowance",
    },
  ],
};

function actions(over: Partial<DaemonActions> = {}): DaemonActions {
  return {
    available: true,
    refresh: vi.fn(async () => undefined),
    save: vi.fn(async () => null),
    setPaused: vi.fn(async () => null),
    setAllPaused: vi.fn(async () => null),
    remove: vi.fn(async () => null),
    ...over,
  };
}

describe("Daemons card", () => {
  beforeEach(() => daemonsSlice.set({ view: null, loaded: true, error: null, runner: { running: null, blockedReason: null, error: null }, replies: {} }));

  it("says daemons need the desktop app in the web preview", () => {
    const el = render(<DaemonsPanel actions={actions({ available: false })} />);
    expect($(el, "daemons-panel").textContent).toContain("Daemons run in the desktop app.");
  });

  it("starts empty: nothing runs until the member creates a daemon", () => {
    const el = render(<DaemonsPanel actions={actions()} />);
    expect($(el, "daemons-panel").textContent).toContain("Nothing runs on a schedule until you create one.");
  });

  it("shows budget exhaustion, estimated tokens and zero spend", () => {
    daemonsSlice.set({ view });
    const el = render(<DaemonsPanel actions={actions()} />);
    expect($(el, "daemon-status").textContent).toBe("budget used up today");
    expect($(el, "daemon-budget").textContent).toContain("4 of 4 runs · 20,100 of 20,000 tokens (estimated) · spend 0 SALT · 3 skipped (budget)");
    expect($(el, "daemon-row").textContent).toContain("over budget");
  });

  it("creates a daemon with the placeholder default budget and spend 0", async () => {
    const a = actions();
    const el = render(<DaemonsPanel actions={a} />);
    act(() => $(el, "daemons-new").click());
    expect($(el, "daemons-form").textContent).toContain("pending owner sign-off");
    const set = (label: string, value: string) => {
      const input = el.querySelector(`[aria-label="${label}"]`) as HTMLInputElement | HTMLTextAreaElement;
      const proto = input instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      act(() => {
        Object.getOwnPropertyDescriptor(proto, "value")?.set?.call(input, value);
        input.dispatchEvent(new Event("input", { bubbles: true }));
      });
    };
    set("Daemon name", "Morning digest");
    set("Task", "Tell me my node height.");
    act(() => ($(el, "daemons-form") as HTMLFormElement).requestSubmit());
    await flush();
    expect(a.save).toHaveBeenCalledWith({
      name: "Morning digest",
      prompt: "Tell me my node height.",
      schedule: "0 9 * * *",
      budget: { maxRunsPerDay: 4, maxTokensPerDay: 20_000, maxTokensPerRun: 6_000, maxSpendSalt: "0" },
    });
  });

  it("surfaces a refused save instead of closing the form", async () => {
    const a = actions({ save: vi.fn(async () => "a daemon needs a name of 1 to 80 characters") });
    const el = render(<DaemonsPanel actions={a} />);
    act(() => $(el, "daemons-new").click());
    act(() => ($(el, "daemons-form") as HTMLFormElement).requestSubmit());
    await flush();
    expect(el.querySelector('[role="alert"]')?.textContent).toContain("needs a name");
    expect($(el, "daemons-form")).not.toBeNull();
  });

  it("pauses one daemon, pauses all, and removes", async () => {
    daemonsSlice.set({ view });
    const a = actions();
    const el = render(<DaemonsPanel actions={a} />);
    act(() => $(el, "daemon-pause").click());
    act(() => $(el, "daemons-pause-all").click());
    act(() => $(el, "daemon-remove").click());
    await flush();
    expect(a.setPaused).toHaveBeenCalledWith("d0123456789abcdef", true);
    expect(a.setAllPaused).toHaveBeenCalledWith(true);
    expect(a.remove).toHaveBeenCalledWith("d0123456789abcdef");
  });

  it("says why runs are held when the local model is not serving", () => {
    daemonsSlice.set({ view, runner: { running: null, blockedReason: "daemons run only on the local model", error: null } });
    const el = render(<DaemonsPanel actions={actions()} />);
    expect($(el, "daemons-blocked").textContent).toContain("daemons run only on the local model");
  });
});

const sources: WidgetSources = {
  context: () => ({ height: 1, peers: 1, finalityAge: 1, nodeState: "synced", staked: 0, liquid: 0, claimable: 0 }),
  model: () => ({ label: "m", id: null }),
  daemons: () => ({ allPaused: false, total: 0, running: 0, paused: 0, budgetUsedUp: 0 }),
};

function widgetsApiFake(): WidgetsApi & { saved: unknown[] } {
  const saved: unknown[] = [];
  return {
    saved,
    list: vi.fn(async () => []),
    save: vi.fn(async (input) => {
      saved.push(input);
      return { id: "0123456789abcdef", name: input.name, description: input.description, queries: input.queries, author: input.author, createdMs: 1, bytes: input.html.length };
    }),
    remove: vi.fn(async () => undefined),
    source: vi.fn(async () => "<p>x</p>"),
  };
}

describe("Widgets card", () => {
  beforeEach(() => widgetsSlice.set({ list: [], loaded: true, error: null }));

  it("says widgets need the desktop app in the web preview", () => {
    const el = render(<WidgetsPanel sources={sources} api={null} />);
    expect($(el, "widgets-panel").textContent).toContain("Widgets run in the desktop app.");
  });

  it("adds a gallery template through the save path, as a gallery widget", async () => {
    const api = widgetsApiFake();
    const el = render(<WidgetsPanel sources={sources} api={api} />);
    act(() => $(el, "widgets-gallery-toggle").click());
    expect($$(el, "widgets-template")).toHaveLength(WIDGET_TEMPLATES.length);
    act(() => ($$(el, "widgets-template")[0].querySelector("button") as HTMLButtonElement).click());
    await flush();
    expect(api.saved[0]).toMatchObject({ name: WIDGET_TEMPLATES[0].name, queries: WIDGET_TEMPLATES[0].queries, author: "gallery" });
  });

  it("renders each saved widget as a sandboxed tile that says what it reads", () => {
    widgetsSlice.set({ list: [{ id: "0123456789abcdef", name: "Block height", description: "", queries: ["node.status"], author: "hermes", createdMs: 1, bytes: 10 }] });
    const el = render(<WidgetsPanel sources={sources} api={widgetsApiFake()} />);
    const tile = $(el, "widget-tile");
    expect(tile.querySelector("iframe")?.getAttribute("sandbox")).toBe("allow-scripts");
    expect(tile.textContent).toContain("by Hermes");
    expect(tile.textContent).toContain("your node's height, peers and sync state");
    expect(tile.textContent).toContain("No network, no app commands.");
  });
});
