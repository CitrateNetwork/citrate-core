// HUP-S2.9 — the send path: a file change the sidecar reports during a turn becomes an Undo card
// on that turn's agent reply, and the undo goes through the bridge (the sim bridge here, which has
// no sidecar and says so: the card shows the refusal, never "undone").
import { describe, it, expect, beforeEach } from "vitest";
import { store } from "./store";
import { turnActivity, IDLE_ACTIVITY } from "./slices/turnActivity";
import { agentUndo, resetAgentUndo, undoChange } from "./slices/agentUndo";
import { bridge } from "../bridge";
import type { ChatProvider } from "../agent/harness";

beforeEach(() => {
  store.setState({ chatMsgs: [], chatStatus: "ready" });
  turnActivity.set(IDLE_ACTIVITY);
  resetAgentUndo();
});

describe("HUP-S2.9 file changes in the chat", () => {
  it("attaches the change to the agent reply of the turn that made it", async () => {
    store.provider = {
      kind: "sidecar",
      label: "test",
      async send({ callbacks }) {
        callbacks.onStatus("thinking");
        callbacks.onActivity?.({ kind: "file_change", change: { session: "s9-beef", seq: 1, tool: "fs_write", paths: ["/w/notes.md"] } });
        callbacks.onToken("Saved your notes.");
        return { role: "assistant", content: "Saved your notes." };
      },
    } as ChatProvider;
    await store.sendChat("save my notes");
    const reply = store.state.chatMsgs.find((m) => m.who !== "You");
    const cards = agentUndo.get().cards;
    expect(cards).toHaveLength(1);
    expect(cards[0]).toMatchObject({ msgId: reply?.id, seq: 1, state: "applied" });
    expect(agentUndo.get().session).toBe("s9-beef");

    await undoChange(bridge.agentHarness, "s9-beef", 1);
    const after = agentUndo.get().cards[0];
    expect(after.state).toBe("refused");
    expect(after.note).toContain("needs the desktop app");
  });
});
