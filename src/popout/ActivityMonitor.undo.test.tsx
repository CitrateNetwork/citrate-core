// HUP-S2.9 — the Activity monitor lists the agent session's recent file changes with Undo for each
// and Undo all; when undo is not available it says why. The pop-out only asks; the main window runs it.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ActivityMonitor } from "./ActivityMonitor";
import { buildMonitorSnapshot } from "./monitorSnapshot";
import type { UndoPanel } from "./undoPanel";
import { IDLE_ACTIVITY } from "../shell/slices/turnActivity";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const snap = buildMonitorSnapshot({ activity: IDLE_ACTIVITY, providerKind: "sidecar", providerLabel: "Hermes", modelLabel: "Gemma", modelId: null, tier: "T1", localCtxTokens: 8192, now: 1 });
const panel: UndoPanel = {
  session: "s4-cafe",
  enabled: true,
  note: null,
  busy: false,
  steps: [
    { seq: 2, status: "committed", paths: ["src/lib.rs"] },
    { seq: 1, status: "undone", paths: ["notes.md"] },
  ],
  last: { ok: false, text: "Not undone: src/lib.rs changed after the agent's edit." },
};

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
const q = (el: HTMLElement, id: string) => el.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

describe("HUP-S2.9 Activity monitor undo", () => {
  it("lists recent steps with Undo, disabled for an undone one, and Undo all", () => {
    const onUndo = vi.fn();
    const el = render(<ActivityMonitor snapshot={snap} now={1} onStop={vi.fn()} undo={panel} onUndo={onUndo} />);
    const rows = el.querySelectorAll('[data-testid="mon-undo-row"]');
    expect(rows.length).toBe(2);
    expect(rows[0].textContent).toContain("src/lib.rs");
    const buttons = el.querySelectorAll('[data-testid="mon-undo-step"]') as NodeListOf<HTMLButtonElement>;
    expect(buttons[0].disabled).toBe(false);
    expect(buttons[1].disabled).toBe(true);
    act(() => buttons[0].click());
    expect(onUndo).toHaveBeenCalledWith("s4-cafe", 2);
    act(() => (q(el, "mon-undo-all") as HTMLButtonElement).click());
    expect(onUndo).toHaveBeenCalledWith("s4-cafe", null);
    expect(q(el, "mon-undo-last")?.textContent).toContain("changed after the agent's edit");
  });

  it("while an undo runs, every Undo is disabled", () => {
    const el = render(<ActivityMonitor snapshot={snap} now={1} onStop={vi.fn()} undo={{ ...panel, busy: true }} onUndo={vi.fn()} />);
    expect((q(el, "mon-undo-all") as HTMLButtonElement).disabled).toBe(true);
    const buttons = el.querySelectorAll('[data-testid="mon-undo-step"]') as NodeListOf<HTMLButtonElement>;
    expect(buttons[0].disabled).toBe(true);
  });

  it("says why when undo is not available, with no buttons", () => {
    const el = render(
      <ActivityMonitor snapshot={snap} now={1} onStop={vi.fn()} undo={{ ...panel, enabled: false, steps: [], last: null, note: "undo checkpoints are not enabled in this agent sidecar" }} onUndo={vi.fn()} />,
    );
    expect(q(el, "mon-undo-note")?.textContent).toContain("not enabled");
    expect(q(el, "mon-undo-all")).toBeNull();
  });

  it("without an undo panel the monitor renders as before", () => {
    const el = render(<ActivityMonitor snapshot={snap} now={1} onStop={vi.fn()} />);
    expect(q(el, "mon-undo")).toBeNull();
  });
});
