// 0.5.4 Hermes x FL WP-H5: fl_round_draft through the store's real handleTool, over the real board
// 07 draft store (localStorage in jsdom) and the comms groups spied at the bridge. A draft escrows
// nothing, so Hermes saves one with no card, but never replaces the member's own draft without
// their Approve. Written red-first.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { store } from "../shell/store";
import { bridge } from "../bridge";
import { AGENT_TOOLS, DRAFT_ONLY_AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall } from "./harness";
import { annotationFor } from "./toolAnnotations";
import { loadRoundDraft, saveRoundDraft, BLANK_DRAFT } from "../fl/roundDraft";
import { draftFieldsOf } from "./flRoundDraftTool";

const call = (args: Record<string, unknown> = {}): ToolCall => ({ id: "c1", name: "fl_round_draft", arguments: JSON.stringify(args) });
const noop = () => {};
const GROUP = { id: "g-research", name: "Research", owner: "o", kind: "channel" as const, members: [] };

beforeEach(() => {
  localStorage.clear();
  store.setState({ chatMsgs: [], queue: [], cerPhase: "review", walletReview: null });
  vi.spyOn(store, "toast").mockImplementation(() => {});
});
afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe("fl_round_draft schema and lists", () => {
  it("is offered, annotated write, and reviewed as draft-only (not read-only)", () => {
    expect(AGENT_TOOLS.map((t) => t.function.name)).toContain("fl_round_draft");
    expect(annotationFor("fl_round_draft")).toEqual({ effect: "write", trust: "trusted" });
    expect(DRAFT_ONLY_AGENT_TOOLS.has("fl_round_draft")).toBe(true);
    expect(READ_ONLY_AGENT_TOOLS.has("fl_round_draft")).toBe(false);
  });
});

describe("fl_round_draft", () => {
  it("with no draft and no fields says there is none", async () => {
    expect(await store.handleTool(call(), "m1", noop)).toMatch(/no round draft on this device yet/);
  });

  it("saves a Hermes draft with no card, marks it Hermes, toasts, and reports what it covers and lacks", async () => {
    vi.spyOn(bridge.groups, "list").mockResolvedValue([GROUP]);
    const sig = vi.spyOn(store, "requestSig");
    const toast = vi.spyOn(store, "toast").mockImplementation(() => {});
    const out = await store.handleTool(
      call({ groupId: "g-research", template: "tune", dataClass: "support chats", budgetSalt: "600", rateSalt: "1.50", maxWorkers: 8, draftedFor: "@ana" }),
      "m1",
      noop,
    );
    expect(sig).not.toHaveBeenCalled();
    const d = loadRoundDraft()!;
    expect(d.source).toBe("hermes");
    expect(d.groupId).toBe("g-research");
    expect(d.dataClass).toBe("support chats");
    expect(d.draftedFor).toBe("@ana");
    expect(toast).toHaveBeenCalledWith(expect.stringMatching(/Open a round/));
    expect(out).toContain("which covers 400 leases");
    expect(out).toContain("Set a deadline");
    expect(out).toMatch(/Hermes cannot sign/);
    expect(out).not.toContain("Research"); // group names are other members' text; only the id is echoed
  });

  it("reads the saved draft back with no fields", async () => {
    saveRoundDraft({ ...BLANK_DRAFT, groupId: "g1", dataClass: "notes", deadline: "2026-12-01" });
    const out = await store.handleTool(call(), "m1", noop);
    expect(out).toContain("written by the member");
    expect(out).toContain("- data class: notes");
    expect(out).toContain("- deadline: 2026-12-01");
  });

  it("refuses a group the member is not in, and saves nothing", async () => {
    vi.spyOn(bridge.groups, "list").mockResolvedValue([GROUP]);
    const out = await store.handleTool(call({ groupId: "g-other", dataClass: "x" }), "m1", noop);
    expect(out).toMatch(/not in a group with id g-other/);
    expect(loadRoundDraft()).toBeNull();
  });

  it("says so when the groups cannot be read", async () => {
    vi.spyOn(bridge.groups, "list").mockRejectedValue(new Error("comms daemon not running"));
    expect(await store.handleTool(call({ groupId: "g1" }), "m1", noop)).toMatch(/couldn't read the member's groups.*comms daemon not running/);
    expect(loadRoundDraft()).toBeNull();
  });

  it("refuses malformed values instead of coercing them", async () => {
    const out = await store.handleTool(call({ budgetSalt: "lots", maxWorkers: 2.5, deadline: "next week", who: "everyone", coordinator: "ftp://x" }), "m1", noop);
    expect(out).toMatch(/Nothing was saved/);
    expect(out).toMatch(/budgetSalt must be a SALT amount/);
    expect(out).toMatch(/maxWorkers must be a whole number/);
    expect(out).toMatch(/deadline must be a date/);
    expect(out).toMatch(/who must be one of/);
    expect(out).toMatch(/coordinator must be an http/);
    expect(loadRoundDraft()).toBeNull();
    expect(draftFieldsOf({ coordinator: "none" }).fields.coordinator).toBeNull();
  });

  it("never replaces the member's own draft without their Approve", async () => {
    saveRoundDraft({ ...BLANK_DRAFT, dataClass: "mine", source: "member" });
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    const out = await store.handleTool(call({ dataClass: "hermes idea" }), "m1", noop);
    expect(sig).toHaveBeenCalledTimes(1);
    const rows = sig.mock.calls[0][0].rows.map((r) => r.k + "=" + r.v).join("\n");
    expect(rows).toContain("dataClass=mine to hermes idea");
    expect(out).toMatch(/kept their own draft/);
    expect(loadRoundDraft()!.dataClass).toBe("mine");
  });

  it("replaces the member's draft after Approve, keeping the fields Hermes did not touch", async () => {
    saveRoundDraft({ ...BLANK_DRAFT, dataClass: "mine", deadline: "2026-12-01", source: "member" });
    vi.spyOn(store, "requestSig").mockResolvedValue("approved");
    await store.handleTool(call({ dataClass: "hermes idea" }), "m1", noop);
    const d = loadRoundDraft()!;
    expect(d.dataClass).toBe("hermes idea");
    expect(d.deadline).toBe("2026-12-01");
    expect(d.source).toBe("hermes");
  });

  it("updates its own earlier draft with no card", async () => {
    saveRoundDraft({ ...BLANK_DRAFT, dataClass: "first", source: "hermes" });
    const sig = vi.spyOn(store, "requestSig");
    await store.handleTool(call({ dataClass: "second" }), "m1", noop);
    expect(sig).not.toHaveBeenCalled();
    expect(loadRoundDraft()!.dataClass).toBe("second");
  });

  it("says so when the device refuses the write", async () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    expect(await store.handleTool(call({ dataClass: "x" }), "m1", noop)).toMatch(/couldn't save the draft on this device: QuotaExceededError/);
  });
});
