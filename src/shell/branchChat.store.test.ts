// FL design board 01: edit, retry and branch keep the original conversation, and the worker-terms
// proposal card is decided only by the member.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import { archivable, chatHistory } from "./slices/chatHistory";
import * as terms from "../fl/workerTerms";
import type { ChatProvider } from "../agent/harness";
import type { ChatMsg } from "./state";
import { journalAttachment } from "../agent/chatAttachments";

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
    // The new Hermes session opens with the earlier messages as normal earlier turns (the
    // provider's open gets them as `history`, sidecarProvider.steer.test.ts), never in the prompt.
    expect(store.branchHistory()).toEqual([
      { role: "user", content: "Network status" },
      { role: "assistant", content: "Synced at block 10." },
    ]);
    expect(store.sidecarSystemPrompt()).not.toContain("Network status");
    expect(store.sidecarSystemPrompt()).not.toContain("Synced at block 10.");
    // Another conversation on screen gets no seed.
    store.setState({ chatThreadId: "tother" });
    expect(store.branchHistory()).toBeNull();
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

describe("board 01 review fixes: proposals and branches", () => {
  it("proposal ids are unique (never p1 again after a relaunch)", () => {
    store.proposeWorkerTerms(TERMS, "a1");
    store.proposeWorkerTerms(TERMS, "a2");
    const ids = store.state.chatMsgs.filter((m) => m.proposal).map((m) => m.proposal!.id);
    expect(new Set(ids).size).toBe(2);
    for (const id of ids) expect(id).not.toMatch(/^p\d+$/);
  });

  it("data classes must be on the allowed list", () => {
    expect(store.proposeWorkerTerms({ ...TERMS, dataClasses: ["medical-records"] })).toMatch(/^refused: Data class "medical-records" is not one this device has/);
    expect(store.proposeWorkerTerms({ ...TERMS, dataClasses: ["hermes-conversations", "hermes-conversations"] })).toMatch(/listed twice/);
    expect(store.proposeWorkerTerms({ ...TERMS, dataClasses: ["hermes-conversations"] })).toMatch(/^The proposal is on a card/);
  });

  it("a confirmed card keeps core's save receipt", async () => {
    vi.spyOn(terms, "saveWorkerTerms").mockResolvedValue({ savedAt: 1794800000000, termsHash: "0x" + "ab".repeat(32), sealed: true });
    store.proposeWorkerTerms(TERMS, "a2");
    const id = store.state.chatMsgs.find((m) => m.id === "a2")!.proposal!.id;
    await store.confirmProposal(id);
    expect(store.state.chatMsgs.find((m) => m.id === "a2")!.proposal).toMatchObject({ state: "confirmed", at: 1794800000000, receipt: { sealed: true } });
  });

  it("an open card is expired in the sealed history and in a branch, and cannot be reopened", async () => {
    store.proposeWorkerTerms(TERMS, "a2");
    const kept = archivable(store.state.chatMsgs).find((m) => m.proposal)!;
    expect(kept.proposal!.state).toBe("expired");
    await store.branchChat("a2", { includeFrom: true });
    const p = store.state.chatMsgs.find((m) => m.proposal)!.proposal!;
    expect(p.state).toBe("expired");
    store.reopenProposal(p.id);
    await store.confirmProposal(p.id);
    expect(store.state.chatMsgs.find((m) => m.proposal)!.proposal!.state).toBe("expired");
  });

  it("a branch records its parent thread in the sealed history", async () => {
    const orig = store.state.chatThreadId;
    await store.branchChat("u2", { text: "again" });
    store.archiveChat();
    const t = chatHistory.get().threads.find((x) => x.id === store.state.chatThreadId);
    expect(t?.parent).toEqual({ threadId: orig, atIndex: 2 });
  });

  it("the worker_terms_propose tool puts a checked card in the chat; it never saves", async () => {
    const save = vi.spyOn(terms, "saveWorkerTerms");
    const out = await store.handleTool(
      { id: "w1", name: "worker_terms_propose", arguments: JSON.stringify({ floorSaltPerUnit: "1.20", hoursStart: "23:00", hoursEnd: "07:00", weekdaysOnly: true, mainsOnly: true, dataClasses: [] }) } as never,
      "a2",
      () => undefined,
    );
    expect(out).toContain("Nothing changes until the member confirms it there");
    expect(store.state.chatMsgs.find((m) => m.id === "a2")!.proposal).toMatchObject({ state: "pending", terms: { floorSaltPerUnit: "1.20", hours: { start: "23:00", end: "07:00" } } });
    expect(save).not.toHaveBeenCalled();
    const bad = await store.handleTool({ id: "w2", name: "worker_terms_propose", arguments: JSON.stringify({ floorSaltPerUnit: "lots" }) } as never, "a1", () => undefined);
    expect(bad).toMatch(/^refused:/);
  });

  it("Retry and edit keep the original message's attachments; a failed reply's Retry does too", async () => {
    const att = journalAttachment({ id: "j1", title: "Node log", kind: "page", pinned: false, blocks: ["held overnight"] });
    const send = vi.spyOn(store, "sendChat").mockResolvedValue();
    store.setState({ chatMsgs: [msg("u1", "You", "summarize this page"), msg("a1", "Agent", "Summary.")] });
    store.setState((s) => ({ chatMsgs: s.chatMsgs.map((m) => (m.id === "u1" ? { ...m, attachments: [att] } : m)) }));
    await store.retryAsBranch("a1");
    expect(send).toHaveBeenLastCalledWith("summarize this page", { attachments: [att] });
    store.setState({ chatMsgs: [{ ...msg("u1", "You", "summarize this page"), attachments: [att] }, msg("a1", "Agent", "Summary.")] });
    await store.branchChat("u1", { text: "summarize it briefly" });
    expect(send).toHaveBeenLastCalledWith("summarize it briefly", { attachments: [att] });
    store.setState({ chatMsgs: [{ ...msg("u1", "You", "summarize this page"), attachments: [att] }, { ...msg("e1", "Agent", ""), error: "connection refused", retryText: "summarize this page" }] });
    await store.retryChat("e1");
    expect(send).toHaveBeenLastCalledWith("summarize this page", { attachments: [att] });
  });
});
