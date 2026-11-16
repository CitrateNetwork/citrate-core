// #690: context the member turns off for one chat is refused by core; attachments reach Hermes.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import type { ChatProvider } from "../agent/harness";
import { datasetAttachment } from "../agent/chatAttachments";

const fakeProvider = (send: ChatProvider["send"]): ChatProvider => ({ kind: "sidecar", label: "test", send }) as unknown as ChatProvider;
const DS = { name: "1794800000000.jsonl", builtMs: 1794800000000, bytes: 1000, examples: 3, sha256: "cd".repeat(32) };

beforeEach(() => store.setState({ chatMsgs: [], chatStatus: "ready", chatQueue: [], chatContextOff: [], chatThreadId: "tctx-" + Math.random().toString(36).slice(2) }));
afterEach(() => {
  vi.restoreAllMocks();
  store.provider = null;
});

describe("#690 context toggles", () => {
  it("a kind turned off is refused before anything runs, as a structured refusal", async () => {
    const list = vi.spyOn(bridge.groups, "list");
    store.setChatContext("groups", false);
    expect(store.state.chatContextOff).toEqual(["groups"]);
    const out = await store.handleTool({ id: "g1", name: "groups_list", arguments: "{}" } as never, "m1", () => undefined);
    expect(out).toMatch(/^declined: the member turned off Your groups for this chat/);
    expect(list).not.toHaveBeenCalled();
    expect(store.takeCallRefusal("g1")).toBe(true);
    store.setChatContext("groups", true);
    expect(store.state.chatContextOff).toEqual([]);
  });

  it("Hermes is told what is off and what is attached, in front of the member's words", async () => {
    let seen = "";
    store.provider = fakeProvider(async ({ messages, callbacks }) => {
      seen = messages[messages.length - 1].content;
      callbacks.onToken("ok");
      return { role: "assistant", content: "" };
    });
    store.setChatContext("chain", false);
    await store.sendChat("Is my dataset ready for a round?", { attachments: [datasetAttachment(DS)] });
    expect(seen).toContain("The member turned off for this chat: Network reads.");
    expect(seen).toContain("Dataset 1794800000000.jsonl");
    expect(seen.endsWith("The member's message:\nIs my dataset ready for a round?")).toBe(true);
    // The transcript shows the member's words, with the attachment as a chip.
    const you = store.state.chatMsgs.find((m) => m.who === "You")!;
    expect(you.text).toBe("Is my dataset ready for a round?");
    expect(you.attachments?.[0].kind).toBe("dataset");
  });

  it("a new chat starts with everything on", () => {
    store.setChatContext("memory", false);
    vi.spyOn(store, "rebuildProvider").mockResolvedValue();
    store.newChat();
    expect(store.state.chatContextOff).toEqual([]);
  });

  it("a follow-up queued with an attachment stays with its conversation", async () => {
    vi.spyOn(store, "rebuildProvider").mockResolvedValue();
    store.queueChat("and this page", [datasetAttachment(DS)]);
    expect(store.state.chatQueue[0]).toMatchObject({ threadId: store.state.chatThreadId, attachments: [expect.objectContaining({ kind: "dataset" })] });
    store.newChat();
    expect(store.state.chatQueue).toEqual([]);
  });
});
