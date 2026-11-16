// FL boards 01 and 07: Hermes's round draft reaches Open a round only through the member's click.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import { clearRoundDraft, loadRoundDraft } from "../fl/roundDraft";
import { archivable } from "./slices/chatHistory";
import type { ChatMsg } from "./state";

const msg = (id: string, who: string, text: string): ChatMsg => ({ id, who, text, chips: [], streaming: false });

beforeEach(() => {
  clearRoundDraft();
  store.setState({ chatStatus: "ready", chatQueue: [], chatMsgs: [msg("u1", "You", "Draft a round"), msg("a1", "Agent", "Here is a draft.")] });
});
afterEach(() => {
  vi.restoreAllMocks();
  clearRoundDraft();
});

const call = (args: Record<string, unknown>) => ({ id: "r1", name: "round_draft_propose", arguments: JSON.stringify(args) }) as never;

describe("round_draft_propose", () => {
  it("puts a draft card in the chat and saves nothing", async () => {
    const out = await store.handleTool(call({ template: "tune", dataClass: "hermes-conversations", budgetSalt: "600", rateSalt: "1.50", maxWorkers: 8 }), "a1", () => undefined);
    expect(out).toContain("Nothing is saved or opened until the member presses Review in Open a round");
    expect(out).toContain("Still to fill in: Choose a group; Set a deadline");
    const c = store.state.chatMsgs.find((m) => m.id === "a1")!.roundDraft!;
    expect(c).toMatchObject({ state: "pending", draft: { template: "tune", budgetSalt: "600", rateSalt: "1.50", maxWorkers: 8 } });
    expect(loadRoundDraft()).toBeNull();
  });

  it("the member's Review hands it to Open a round as Hermes's draft", async () => {
    const go = vi.spyOn(store, "go").mockImplementation(() => undefined);
    await store.handleTool(call({ template: "eval", deadline: "2026-12-01" }), "a1", () => undefined);
    const id = store.state.chatMsgs.find((m) => m.id === "a1")!.roundDraft!.id;
    store.openRoundDraft(id);
    expect(go).toHaveBeenCalledWith("groups/rounds/open");
    expect(loadRoundDraft()).toMatchObject({ template: "eval", deadline: "2026-12-01", source: "hermes", draftedFor: "this Hermes chat" });
    expect(store.state.chatMsgs.find((m) => m.id === "a1")!.roundDraft!.state).toBe("opened");
  });

  it("Not now saves nothing; a restored card is expired and cannot be opened", async () => {
    await store.handleTool(call({ template: "tune" }), "a1", () => undefined);
    const card = store.state.chatMsgs.find((m) => m.id === "a1")!.roundDraft!;
    expect(archivable(store.state.chatMsgs).find((m) => m.roundDraft)!.roundDraft!.state).toBe("expired");
    store.declineRoundDraft(card.id);
    store.openRoundDraft(card.id);
    expect(loadRoundDraft()).toBeNull();
    expect(store.state.chatMsgs.find((m) => m.id === "a1")!.roundDraft!.state).toBe("declined");
  });

  it("drops arguments of the wrong type instead of guessing", async () => {
    await store.handleTool(call({ template: "launch", who: "everyone", maxWorkers: 2.5, budgetSalt: "" }), "a1", () => undefined);
    expect(store.state.chatMsgs.find((m) => m.id === "a1")!.roundDraft!.draft).toEqual({});
  });
});
