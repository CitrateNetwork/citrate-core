// FL design board 01: edit, retry and branch keep the original conversation, and the worker-terms
// proposal card is decided only by the member.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import { chatHistory } from "./slices/chatHistory";
import * as terms from "../fl/workerTerms";
import type { ChatProvider } from "../agent/harness";
import type { ChatMsg } from "./state";

const fakeProvider = (send: ChatProvider["send"]): ChatProvider => ({ kind: "sidecar", label: "test", send }) as unknown as ChatProvider;
const msg = (id: string, who: string, text: string): ChatMsg => ({ id, who, text, chips: [], streaming: false });
const TERMS: terms.WorkerTerms = { floorSaltPerUnit: "1.20", hours: { start: "23:00", end: "07:00" }, weekdaysOnly: true, mainsOnly: true, dataClasses: [] };

let sent: string[] = [];
beforeEach(() => {
  sent = [];
  store.setState({
    chatStatus: "ready",
    chatQueue: [],
    chatThreadId: "torig-" + Math.random().toString(36).slice(2),
    chatMsgs: [msg("u1", "You", "Network status"), msg("a1", "Agent", "Synced at block 10."), msg("u2", "You", "Put my GPU to work on rounds at night."), msg("a2", "Agent", "Group rounds not live yet.")],
  });
  vi.spyOn(store, "rebuildProvider").mockImplementation(async () => {
    store.provider = fakeProvider(async ({ messages, callbacks }) => {
      sent.push(messages[messages.length - 1].content);
      callbacks.onToken("ok");
      return { role: "assistant", content: "" };
    });
  });
});
afterEach(() => {
  vi.restoreAllMocks();
  store.provider = null;
});

describe("board 01: edit and branch", () => {
  it("editing a past message branches: the earlier messages are kept, the edit is sent, the original stays", async () => {
    const orig = store.state.chatThreadId;
    const record = vi.spyOn(chatHistory, "set");
    expect(await store.branchChat("u2", { text: "Put my GPU to work on rounds, weeknights only." })).toBe(true);
    expect(store.state.chatThreadId).not.toBe(orig);
    expect(store.state.chatMsgs.map((m) => m.text).slice(0, 3)).toEqual(["Network status", "Synced at block 10.", "Put my GPU to work on rounds, weeknights only."]);
    expect(sent).toEqual(["Put my GPU to work on rounds, weeknights only."]);
    // The original thread was archived before the switch.
    expect(record).toHaveBeenCalled();
    // The new Hermes session hears the earlier messages as context.
    expect(store.sidecarSystemPrompt()).toContain("Member: Network status");
    expect(store.sidecarSystemPrompt()).toContain("Hermes: Synced at block 10.");
    expect(store.sidecarSystemPrompt()).not.toContain("Put my GPU to work on rounds at night.");
  });

  it("Retry sends the answer's member message again in a branch", async () => {
    expect(await store.retryAsBranch("a2")).toBe(true);
    expect(sent).toEqual(["Put my GPU to work on rounds at night."]);
    expect(store.state.chatMsgs.filter((m) => m.who === "You").map((m) => m.text)).toEqual(["Network status", "Put my GPU to work on rounds at night."]);
  });

  it("Branch from here keeps the answer and sends nothing", async () => {
    await store.branchChat("a1", { includeFrom: true });
    expect(store.state.chatMsgs.map((m) => m.text)).toEqual(["Network status", "Synced at block 10."]);
    expect(sent).toEqual([]);
  });

  it("never branches while a run is live", async () => {
    store.setState({ chatStatus: "thinking" });
    const orig = store.state.chatThreadId;
    expect(await store.branchChat("u2", { text: "x" })).toBe(false);
    expect(store.state.chatThreadId).toBe(orig);
  });
});

describe("board 01: the worker-terms proposal card", () => {
  it("malformed terms are refused and no card is shown", () => {
    const r = store.proposeWorkerTerms({ ...TERMS, floorSaltPerUnit: "lots" });
    expect(r).toMatch(/^refused: The floor must be a SALT amount/);
    expect(store.state.chatMsgs.some((m) => m.proposal)).toBe(false);
  });

  it("Confirm in this build: saving is refused, so the card says nothing changed", async () => {
    store.proposeWorkerTerms(TERMS, "a2");
    const p = store.state.chatMsgs.find((m) => m.id === "a2")!.proposal!;
    expect(p.state).toBe("pending");
    await store.confirmProposal(p.id);
    const after = store.state.chatMsgs.find((m) => m.id === "a2")!.proposal!;
    expect(after.state).toBe("failed");
    expect(after.error).toBe(terms.TERMS_SAVE_UNAVAILABLE);
  });

  it("Confirm once storage saves: confirmed by you, with the time", async () => {
    const save = vi.spyOn(terms, "saveWorkerTerms").mockResolvedValue({ savedAt: 1, termsHash: "0xab", sealed: true });
    store.proposeWorkerTerms(TERMS);
    const id = store.state.chatMsgs.find((m) => m.proposal)!.proposal!.id;
    await store.confirmProposal(id);
    expect(save).toHaveBeenCalledWith(TERMS);
    const p = store.state.chatMsgs.find((m) => m.proposal)!.proposal!;
    expect(p.state).toBe("confirmed");
    expect(typeof p.at).toBe("number");
  });

  it("Not now changes nothing and can be reviewed again", () => {
    const save = vi.spyOn(terms, "saveWorkerTerms");
    store.proposeWorkerTerms(TERMS);
    const id = store.state.chatMsgs.find((m) => m.proposal)!.proposal!.id;
    store.declineProposal(id);
    expect(store.state.chatMsgs.find((m) => m.proposal)!.proposal!.state).toBe("declined");
    store.reopenProposal(id);
    expect(store.state.chatMsgs.find((m) => m.proposal)!.proposal!.state).toBe("pending");
    expect(save).not.toHaveBeenCalled();
  });
});
