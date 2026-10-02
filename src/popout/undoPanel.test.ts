// HUP-S2.9 — the Activity monitor's undo panel travels over the typed pop-out bridge. Both new
// messages are validated whole on receipt; a malformed one is dropped.
import { describe, it, expect } from "vitest";
import { parseToMain, parseToPopout } from "./bridge";
import { isUndoPanel, type UndoPanel } from "./undoPanel";

const panel: UndoPanel = {
  session: "s4-cafe",
  enabled: true,
  note: null,
  busy: false,
  steps: [{ seq: 2, status: "committed", paths: ["notes.md"] }, { seq: 1, status: "undone", paths: ["a.txt", "b.txt"] }],
  last: { ok: true, text: "Undone: restored a.txt." },
};

describe("HUP-S2.9 undo panel messages", () => {
  it("validates a panel", () => {
    expect(isUndoPanel(panel)).toBe(true);
    expect(isUndoPanel({ ...panel, session: null, enabled: false, steps: [], last: null, note: "No agent file changes yet." })).toBe(true);
    expect(isUndoPanel({ ...panel, steps: [{ seq: "2", status: "committed", paths: [] }] })).toBe(false);
    expect(isUndoPanel({ ...panel, steps: [{ seq: 2, status: "exploded", paths: [] }] })).toBe(false);
    expect(isUndoPanel({ ...panel, last: { ok: "yes", text: "" } })).toBe(false);
    expect(isUndoPanel({ ...panel, busy: undefined })).toBe(false);
    expect(isUndoPanel({ ...panel, steps: new Array(101).fill({ seq: 1, status: "committed", paths: [] }) })).toBe(false);
  });

  it("main → monitor: the panel message", () => {
    expect(parseToPopout({ v: 1, type: "monitor.undo", panel })).toEqual({ v: 1, type: "monitor.undo", panel });
    expect(parseToPopout({ v: 1, type: "monitor.undo", panel: { ...panel, enabled: "no" } })).toBeNull();
    expect(parseToPopout({ v: 2, type: "monitor.undo", panel })).toBeNull();
  });

  it("monitor → main: an undo request for one step or the whole session", () => {
    expect(parseToMain({ v: 1, type: "monitor.undo.request", session: "s4-cafe", seq: 2 })).toEqual({ v: 1, type: "monitor.undo.request", session: "s4-cafe", seq: 2 });
    expect(parseToMain({ v: 1, type: "monitor.undo.request", session: "s4-cafe", seq: null })).toEqual({ v: 1, type: "monitor.undo.request", session: "s4-cafe", seq: null });
    for (const bad of [
      { v: 1, type: "monitor.undo.request", session: "../x", seq: 1 },
      { v: 1, type: "monitor.undo.request", session: "s4-cafe", seq: 0 },
      { v: 1, type: "monitor.undo.request", session: "s4-cafe", seq: 1.5 },
      { v: 1, type: "monitor.undo.request", session: "s4-cafe" },
      { v: 1, type: "monitor.undo.request", seq: 1 },
    ]) expect(parseToMain(bad)).toBeNull();
  });
});
