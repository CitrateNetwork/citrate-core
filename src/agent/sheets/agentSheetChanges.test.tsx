// HUP-S10.2 follow-up (US-10.2) — sheet_write undo from Journal > Sheets.
//
// BDD:
//   Given Hermes wrote no sheet this session, then the Sheets view says so.
//   Given Hermes wrote a sheet (a checkpointed sheet_write), then the Sheets view lists it with
//     Undo, and other file changes are not listed there.
//   When the member presses Undo there, then the sidecar's undo for that step runs and the row
//     says what was restored; a refusal (the file changed since) is shown with its reason and
//     Undo can be tried again.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { AgentSheetChanges, NO_AGENT_SHEETS, sheetWriteCards } from "./AgentSheetChanges";
import { agentUndo, recordFileChange, resetAgentUndo, undoChange, type UndoApi } from "../../shell/slices/agentUndo";
import { parseFileChange, type UndoOutcome } from "../fileChanges";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const sheetResult = (seq: number, path: string) =>
  JSON.stringify({ path, paths: [path], format: "csv", written: true, checkpoint: { session: "s1-ab", seq } });

function recordSheet(seq: number, path: string) {
  const change = parseFileChange("sheet_write", sheetResult(seq, path));
  expect(change).not.toBeNull();
  recordFileChange(change!, "m1");
}

const ok = (restored: string[], undone: number[]): UndoOutcome => ({ ok: true, undone, restored, prunedThrough: null, kind: null, reason: null, conflicts: [] });

function api(outcomes: UndoOutcome[]): UndoApi & { undoStep: ReturnType<typeof vi.fn> } {
  return {
    checkpoints: vi.fn(async (id: string) => ({ session: id, enabled: true, steps: [], note: null })),
    undoStep: vi.fn(async () => outcomes.shift() ?? ok([], [])),
    undoSession: vi.fn(async () => ok([], [])),
  };
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;

/** Mount the list bound to the live undo slice, as Journal > Sheets does. */
function Bound({ a }: { a: UndoApi }) {
  const s = agentUndo.use();
  return <AgentSheetChanges cards={s.cards} onUndo={(session, seq) => void undoChange(a, session, seq)} />;
}

function mount(a: UndoApi) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => root!.render(<Bound a={a} />));
  return host;
}

const $$ = (h: HTMLElement, id: string) => Array.from(h.querySelectorAll(`[data-testid="${id}"]`)) as HTMLElement[];

beforeEach(() => resetAgentUndo());
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("sheets Hermes wrote, in Journal > Sheets", () => {
  it("says no sheet was written yet", () => {
    const h = mount(api([]));
    expect($$(h, "agent-sheet-changes-empty")[0].textContent).toBe(NO_AGENT_SHEETS);
    expect($$(h, "file-change-card")).toHaveLength(0);
  });

  it("lists only sheet writes, newest first", () => {
    recordSheet(1, "/w/q3.csv");
    const fw = parseFileChange("file_write", JSON.stringify({ path: "/w/a.md", paths: ["/w/a.md"], written: true, checkpoint: { session: "s1-ab", seq: 2 } }));
    recordFileChange(fw!, "m1");
    recordSheet(3, "/w/q4.xlsx");
    expect(sheetWriteCards(agentUndo.get().cards).map((c) => c.seq)).toEqual([3, 1]);
    const h = mount(api([]));
    const cards = $$(h, "file-change-card").map((c) => c.textContent ?? "");
    expect(cards).toHaveLength(2);
    expect(cards[0]).toContain("Wrote sheet");
    expect(cards[0]).toContain("/w/q4.xlsx");
    expect(cards[1]).toContain("/w/q3.csv");
  });

  it("undoes a sheet write through the sidecar's undo and says what was restored", async () => {
    recordSheet(4, "/w/q3.csv");
    const a = api([ok(["/w/q3.csv"], [4])]);
    const h = mount(a);
    await act(async () => {
      $$(h, "file-change-undo")[0].click();
    });
    expect(a.undoStep).toHaveBeenCalledWith("s1-ab", 4);
    expect($$(h, "file-change-note")[0].textContent).toBe("Undone: restored q3.csv.");
    expect($$(h, "file-change-undo")).toHaveLength(0);
    expect(agentUndo.get().cards[0].state).toBe("undone");
  });

  it("shows a refusal with its reason and keeps Undo available", async () => {
    recordSheet(5, "/w/q3.csv");
    const refused: UndoOutcome = {
      ok: false,
      undone: [],
      restored: [],
      prunedThrough: null,
      kind: "conflict",
      reason: "changed since",
      conflicts: [{ seq: 5, path: "q3.csv", found: "file sha256:ab" }],
    };
    const a = api([refused]);
    const h = mount(a);
    await act(async () => {
      $$(h, "file-change-undo")[0].click();
    });
    const note = $$(h, "file-change-note")[0];
    expect(note.getAttribute("role")).toBe("alert");
    expect(note.textContent).toMatch(/^Not undone: q3\.csv changed after the agent's edit/);
    expect(($$(h, "file-change-undo")[0] as HTMLButtonElement).disabled).toBe(false);
  });
});
