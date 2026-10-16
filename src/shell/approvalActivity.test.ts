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

describe("a HIC-required everyday write asks once, on its own card", () => {
  // Reviewer (N5-everyday): gsheets_append and schedule_add build their own approval card, so a
  // hic:"required" call must carry the HIC reason on THAT card and never stack a generic one first.
  const HIC = { hic: "required" as const, hicReason: "this session read untrusted content (from gsheets_read)" };
  const args = {
    spreadsheetId: "1AbCdEfGhIjKlMnOpQrStUvWxYz012345",
    range: "Sheet1!A:B",
    rows: [["a", 1]],
    title: "Weekly review",
    start: "2099-01-05T09:00",
  };
  for (const name of ["gsheets_append", "schedule_add"]) {
    it(`${name}: one card with the HIC reason; Approve runs the write once, Decline runs nothing`, async () => {
      const sig = vi.spyOn(store, "requestSig").mockResolvedValueOnce("approved").mockResolvedValue("declined");
      const invoked: string[] = [];
      vi.spyOn(store, "everydayInvoke").mockReturnValue((async (cmd: string) => {
        invoked.push(cmd);
        return cmd === "google_workspace_status" ? [{ service: "gsheets", configured: true, connected: true, note: null }] : {};
      }) as never);
      await store.handleTool({ id: "h1", name, arguments: JSON.stringify(args) }, "m1", () => {}, HIC);
      expect(sig).toHaveBeenCalledTimes(1);
      expect(sig.mock.calls[0][0].hic).toEqual({ reason: HIC.hicReason });
      expect(sig.mock.calls[0][0].card?.tool).toBe(name);
      const writes = () => invoked.filter((c) => c === "gsheets_append" || c === "hermes_schedule_add");
      expect(writes()).toEqual([name === "gsheets_append" ? "gsheets_append" : "hermes_schedule_add"]);
      await store.handleTool({ id: "h2", name, arguments: JSON.stringify(args) }, "m1", () => {}, HIC);
      expect(sig).toHaveBeenCalledTimes(2);
      expect(writes()).toHaveLength(1);
    });
  }
});
