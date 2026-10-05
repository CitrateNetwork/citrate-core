// HUP-S10.2 follow-up (US-10.2) — Journal > Sheets lists the sheets Hermes wrote, each with Undo.
//
// BDD:
//   Given Hermes wrote a sheet in a granted folder this session, when the member opens
//     Journal > Sheets, then that sheet write is listed there with Undo.
//   When the member presses Undo there, then the app's agent undo (hermes_undo_step) runs for
//     that session and step.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Journal } from "./Journal";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";
import { recordFileChange, resetAgentUndo } from "../shell/slices/agentUndo";
import { bridge } from "../bridge";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
beforeEach(() => resetAgentUndo());
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  resetAgentUndo();
  vi.restoreAllMocks();
});

describe("Journal > Sheets", () => {
  it("shows the sheet Hermes wrote with Undo once the Sheets view is open", async () => {
    recordFileChange({ session: "s1-ab", seq: 2, tool: "sheet_write", paths: ["/w/budget.csv"] }, "m1");
    const s = freshState("p1");
    s.route = "journal";
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => {
      root!.render(<Journal store={{} as unknown as Store} s={s} />);
    });
    expect(host.querySelector('[data-testid="agent-sheet-changes"]')).toBeNull();
    await act(async () => {
      (host!.querySelector('[data-testid="j-sheets"]') as HTMLButtonElement).click();
    });
    const list = host.querySelector('[data-testid="agent-sheet-changes"]');
    expect(list?.textContent).toContain("/w/budget.csv");
    const undoStep = vi
      .spyOn(bridge.agentHarness, "undoStep")
      .mockResolvedValue({ ok: true, undone: [2], restored: ["/w/budget.csv"], prunedThrough: null, kind: null, reason: null, conflicts: [] });
    await act(async () => {
      (list!.querySelector('[data-testid="file-change-undo"]') as HTMLButtonElement).click();
    });
    expect(undoStep).toHaveBeenCalledWith("s1-ab", 2);
  });
});
