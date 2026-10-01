// HUP-S2.9 — the undo slice: a file change becomes a card on its chat message; Undo calls the
// sidecar through the bridge and the card shows what happened, including an honest refusal; the
// Activity monitor's panel lists the session's recent steps.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { agentUndo, recordFileChange, refreshUndoPanel, undoChange, undoSession, resetAgentUndo, MAX_CARDS, type UndoApi } from "./agentUndo";
import type { CheckpointList, UndoOutcome } from "../../agent/fileChanges";

const ok = (undone: number[], restored: string[]): UndoOutcome => ({ ok: true, undone, restored, prunedThrough: null, kind: null, reason: null, conflicts: [] });
const refused = (kind: string, reason: string, conflicts: UndoOutcome["conflicts"] = []): UndoOutcome => ({ ok: false, undone: [], restored: [], prunedThrough: null, kind, reason, conflicts });
const list = (steps: CheckpointList["steps"]): CheckpointList => ({ session: "s4-cafe", enabled: true, steps, note: null });

function api(over: Partial<UndoApi> = {}): UndoApi {
  return {
    checkpoints: vi.fn(async () => list([])),
    undoStep: vi.fn(async (_id: string, seq: number) => ok([seq], ["notes.md"])),
    undoSession: vi.fn(async () => ok([2, 1], ["a", "b"])),
    ...over,
  };
}

const fc = (seq: number) => ({ session: "s4-cafe", seq, tool: "fs_write", paths: ["/w/notes.md"] });

beforeEach(() => resetAgentUndo());

describe("HUP-S2.9 agent undo slice", () => {
  it("records a change as an applied card on its message and remembers the session", () => {
    recordFileChange(fc(1), "m2");
    recordFileChange(fc(1), "m2"); // a replayed event is not a second card
    const s = agentUndo.get();
    expect(s.cards).toHaveLength(1);
    expect(s.cards[0]).toMatchObject({ seq: 1, msgId: "m2", state: "applied", note: null });
    expect(s.session).toBe("s4-cafe");
  });

  it("keeps at most MAX_CARDS cards, dropping the oldest", () => {
    for (let i = 1; i <= MAX_CARDS + 3; i++) recordFileChange(fc(i), "m");
    const cards = agentUndo.get().cards;
    expect(cards).toHaveLength(MAX_CARDS);
    expect(cards[0].seq).toBe(4);
  });

  it("an undone step marks its card undone and refreshes the panel", async () => {
    recordFileChange(fc(1), "m2");
    const a = api({ checkpoints: vi.fn(async () => list([{ seq: 1, status: "undone", paths: ["notes.md"], root: "/w" }])) });
    await undoChange(a, "s4-cafe", 1);
    expect(a.undoStep).toHaveBeenCalledWith("s4-cafe", 1);
    const s = agentUndo.get();
    expect(s.cards[0].state).toBe("undone");
    expect(s.cards[0].note).toBe("Undone: restored notes.md.");
    expect(s.panel.steps[0].status).toBe("undone");
    expect(s.panel.last).toEqual({ ok: true, text: "Undone: restored notes.md." });
  });

  it("a conflict leaves the card applied-but-refused with the reason, never undone", async () => {
    recordFileChange(fc(1), "m2");
    const a = api({ undoStep: vi.fn(async () => refused("conflict", "undo refused, nothing was changed", [{ seq: 1, path: "notes.md", found: "file sha256:ab" }])) });
    await undoChange(a, "s4-cafe", 1);
    const c = agentUndo.get().cards[0];
    expect(c.state).toBe("refused");
    expect(c.note).toContain("notes.md changed after the agent's edit");
    expect(agentUndo.get().panel.last?.ok).toBe(false);
  });

  it("a step that was already undone elsewhere (the monitor) shows as undone, with the reason", async () => {
    recordFileChange(fc(1), "m2");
    await undoChange(api({ undoStep: vi.fn(async () => refused("already_undone", "session s4-cafe step 1 is already undone")) }), "s4-cafe", 1);
    expect(agentUndo.get().cards[0]).toMatchObject({ state: "undone", note: "Not undone: session s4-cafe step 1 is already undone" });
  });

  it("a transport failure is shown as a failure, and the card can be tried again", async () => {
    recordFileChange(fc(1), "m2");
    const a = api({ undoStep: vi.fn(async () => { throw new Error("hermes is not running (no session bearer)"); }) });
    await undoChange(a, "s4-cafe", 1);
    const c = agentUndo.get().cards[0];
    expect(c.state).toBe("failed");
    expect(c.note).toContain("hermes is not running");
  });

  it("undo all marks every undone step's card and reports in the panel", async () => {
    recordFileChange(fc(1), "m2");
    recordFileChange(fc(2), "m3");
    const a = api();
    await undoSession(a, "s4-cafe");
    expect(a.undoSession).toHaveBeenCalledWith("s4-cafe");
    expect(agentUndo.get().cards.map((c) => c.state)).toEqual(["undone", "undone"]);
    expect(agentUndo.get().panel.last).toEqual({ ok: true, text: "Undone: restored 2 files." });
  });

  it("undo all marks only the steps the sidecar undid; a pruned step's card is not claimed as undone", async () => {
    recordFileChange(fc(1), "m2");
    recordFileChange(fc(2), "m3");
    const a = api({ undoSession: vi.fn(async () => ({ ...ok([2], ["notes.md"]), prunedThrough: 1 })) });
    await undoSession(a, "s4-cafe");
    expect(agentUndo.get().cards.map((c) => c.state)).toEqual(["applied", "undone"]);
    expect(agentUndo.get().panel.last?.text).toContain("up to step 1, were pruned");
  });

  it("the panel says so when undo is not enabled, and when there is no agent session yet", async () => {
    await refreshUndoPanel(api());
    expect(agentUndo.get().panel).toMatchObject({ enabled: false, steps: [] });
    expect(agentUndo.get().panel.note).toContain("No agent file changes yet");
    recordFileChange(fc(1), "m2");
    await refreshUndoPanel(api({ checkpoints: vi.fn(async () => ({ session: "s4-cafe", enabled: false, steps: [], note: "undo checkpoints are not enabled in this agent sidecar" })) }));
    expect(agentUndo.get().panel).toMatchObject({ enabled: false, note: "undo checkpoints are not enabled in this agent sidecar" });
    await refreshUndoPanel(api({ checkpoints: vi.fn(async () => { throw new Error("hermes is not running"); }) }));
    expect(agentUndo.get().panel.note).toContain("could not be read: hermes is not running");
  });
});
