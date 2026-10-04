// HUP-S7.6 (US-7.4 AC1) — every approval a chat tool asks for shows in the Activity monitor:
// pending while the card is open, then the member's decision.
import { describe, it, expect, vi, afterEach } from "vitest";
import { store } from "./store";
import { IDLE_ACTIVITY, beginTurn, turnActivity } from "./slices/turnActivity";

afterEach(() => {
  vi.restoreAllMocks();
  turnActivity.set(IDLE_ACTIVITY);
});

describe("approvals in the turn activity", () => {
  it("a write tool's card is pending until the member decides", async () => {
    beginTurn("sidecar", "Hermes", 1000);
    let release: (r: string) => void = () => {};
    vi.spyOn(store, "requestSig").mockImplementation(() => new Promise<string>((r) => (release = r)));
    vi.spyOn(store, "everydayInvoke").mockReturnValue((async () => [{ service: "gsheets", configured: true, connected: true, note: null }]) as never);
    const call = { id: "c42", name: "gsheets_append", arguments: JSON.stringify({ spreadsheetId: "1AbCdEfGhIjKlMnOpQrStUvWxYz012345", range: "A:B", rows: [["x"]] }) };
    const done = store.handleTool(call, "m1", () => {});
    await vi.waitFor(() => expect(turnActivity.get().approvals?.map((a) => [a.callId, a.tool, a.state])).toEqual([["c42", "gsheets_append", "pending"]]));
    release("declined");
    await done;
    expect(turnActivity.get().approvals?.map((a) => a.state)).toEqual(["declined"]);
  });

  it("a read tool asks nothing and records no approval", async () => {
    beginTurn("sidecar", "Hermes", 1000);
    const sig = vi.spyOn(store, "requestSig");
    vi.spyOn(store, "everydayInvoke").mockReturnValue((async (cmd: string) => (cmd === "hermes_schedule_list" ? { status: "ok", error: null, entries: [], occurrences: [] } : [])) as never);
    await store.handleTool({ id: "c1", name: "schedule_list", arguments: "{}" }, "m1", () => {});
    expect(sig).not.toHaveBeenCalled();
    expect(turnActivity.get().approvals).toEqual([]);
  });
});
