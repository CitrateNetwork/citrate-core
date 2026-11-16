// FL design board 01: the store writes each reply's run timeline from the provider's real reports,
// and handles Stop, Steer now and Queue for after.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import { TurnStopped, type ChatProvider } from "../agent/harness";

const fakeProvider = (send: ChatProvider["send"]): ChatProvider => ({ kind: "sidecar", label: "test", send }) as unknown as ChatProvider;
const agentMsgs = () => store.state.chatMsgs.filter((m) => m.who !== "You");

beforeEach(() => {
  store.setState({ chatMsgs: [], chatStatus: "ready", chatQueue: [], height: 1204311, chatThreadId: "trun-" + Math.random().toString(36).slice(2) });
});
afterEach(() => {
  vi.restoreAllMocks();
  store.provider = null;
});

describe("board 01: the run timeline of a reply", () => {
  it("records think, core read (with the block), sidecar tool, refused call and draft", async () => {
    vi.spyOn(store, "handleTool").mockResolvedValue('{"height":1204311}');
    store.provider = fakeProvider(async ({ callbacks }) => {
      callbacks.onStatus("thinking");
      callbacks.onActivity?.({ kind: "page", at: 1000 });
      callbacks.onActivity?.({ kind: "step", step: 1 });
      await callbacks.onToolCall({ id: "call_0", name: "node_status", arguments: "{}" } as never);
      callbacks.onActivity?.({ kind: "page", at: 2000 });
      callbacks.onActivity?.({ kind: "tool_start", callId: "call_1", tool: "mcp__fs__read", host: "sidecar" });
      callbacks.onActivity?.({ kind: "page", at: 2500 });
      callbacks.onActivity?.({ kind: "tool_output", callId: "call_1", tool: "mcp__fs__read", status: "ok", content: "x" });
      callbacks.onActivity?.({ kind: "tool_start", callId: "call_2", tool: "contract_deploy", host: "refused" });
      callbacks.onActivity?.({ kind: "tool_end", callId: "call_2", tool: "contract_deploy", status: "denied", content: "declined: needs your approval" });
      callbacks.onStatus("streaming");
      callbacks.onToken("Your node is at block 1,204,311.");
      callbacks.onStatus("done");
      return { role: "assistant", content: "" };
    });
    await store.sendChat("Network status");
    const [m] = agentMsgs();
    expect(m.run?.outcome).toBe("answered");
    expect(m.run?.steps.map((s) => [s.kind, s.tool ?? "", s.state])).toEqual([
      ["think", "", "done"],
      ["read", "node_status", "done"],
      ["tool", "mcp__fs__read", "done"],
      ["tool", "contract_deploy", "refused"],
      ["draft", "", "done"],
    ]);
    const read = m.run!.steps[1];
    expect(read.timing).toBe("core");
    expect(read.block).toBe(1204311);
    // Sidecar tool timed by its pages: started at 2000, result at 2500.
    expect(m.run!.steps[2].durationMs).toBe(500);
    // Refused call: start and result arrived in the same page, so no duration is shown.
    expect(m.run!.steps[3].durationMs).toBeNull();
    expect(m.text).toContain("1,204,311");
    expect(m.streaming).toBe(false);
  });

  it("an unread node height leaves the chain read's block unknown, never 0", async () => {
    store.setState({ height: 0 });
    vi.spyOn(store, "handleTool").mockResolvedValue("{}");
    store.provider = fakeProvider(async ({ callbacks }) => {
      await callbacks.onToolCall({ id: "c", name: "staking_status", arguments: "{}" } as never);
      callbacks.onToken("ok");
      return { role: "assistant", content: "" };
    });
    await store.sendChat("What is my staking position?");
    expect(agentMsgs()[0].run?.steps.find((s) => s.tool === "staking_status")?.block).toBeNull();
  });

  it("Stop ends the run with its steps stopped", async () => {
    store.provider = fakeProvider(({ callbacks, signal }) => {
      callbacks.onStatus("thinking");
      return new Promise((_, reject) => signal?.addEventListener("abort", () => reject(new TurnStopped())));
    });
    const p = store.sendChat("Put my GPU to work on rounds at night.");
    await Promise.resolve();
    expect(store.chatRunLive()).toBe(true);
    store.stopAgentTurn();
    await p;
    const [m] = agentMsgs();
    expect(m.error).toBe("stopped by you");
    expect(m.run?.outcome).toBe("stopped");
    expect(m.run?.steps.every((s) => s.state === "stopped")).toBe(true);
  });
});

describe("board 01: Steer now and Queue for after", () => {
  it("a queued follow-up sends after the run finishes", async () => {
    let release: () => void = () => undefined;
    const sent: string[] = [];
    store.provider = fakeProvider(async ({ messages, callbacks }) => {
      sent.push(messages[messages.length - 1].content);
      if (sent.length === 1) await new Promise<void>((r) => (release = r));
      callbacks.onToken("ok");
      return { role: "assistant", content: "" };
    });
    const first = store.sendChat("Network status");
    await Promise.resolve();
    store.queueChat("also my staking position");
    expect(store.state.chatQueue.map((q) => q.text)).toEqual(["also my staking position"]);
    release();
    await first;
    await vi.waitFor(() => expect(sent).toEqual(["Network status", "also my staking position"]));
    await vi.waitFor(() => expect(store.state.chatStatus).toBe("ready"));
    expect(store.state.chatQueue).toEqual([]);
  });

  it("Steer now stops the live run and sends the steer at once; queued items follow it", async () => {
    const sent: string[] = [];
    store.provider = fakeProvider(({ messages, callbacks, signal }) => {
      sent.push(messages[messages.length - 1].content);
      if (sent.length === 1) return new Promise((_, reject) => signal?.addEventListener("abort", () => reject(new TurnStopped())));
      callbacks.onToken("steered");
      return Promise.resolve({ role: "assistant", content: "" });
    });
    const first = store.sendChat("Put my GPU to work on rounds at night.");
    await Promise.resolve();
    store.queueChat("later question");
    store.steerChat("also skip Fridays");
    // The steer goes ahead of what was queued.
    expect(store.state.chatQueue.map((q) => [q.text, q.steer])).toEqual([
      ["also skip Fridays", true],
      ["later question", false],
    ]);
    await first;
    await vi.waitFor(() => expect(sent).toEqual(["Put my GPU to work on rounds at night.", "also skip Fridays", "later question"]));
    await vi.waitFor(() => expect(store.state.chatStatus).toBe("ready"));
    expect(store.state.chatQueue).toEqual([]);
    expect(agentMsgs()[0].run?.outcome).toBe("stopped");
  });

  it("after a Stop, a queued follow-up waits for the member", async () => {
    store.provider = fakeProvider(({ signal }) => new Promise((_, reject) => signal?.addEventListener("abort", () => reject(new TurnStopped()))));
    const first = store.sendChat("Network status");
    await Promise.resolve();
    store.queueChat("later question");
    store.stopAgentTurn();
    await first;
    expect(store.state.chatQueue.map((q) => q.text)).toEqual(["later question"]);
    store.dropQueuedChat(store.state.chatQueue[0].id);
    expect(store.state.chatQueue).toEqual([]);
  });

  it("ignores empty follow-ups", () => {
    store.queueChat("   ");
    store.steerChat("");
    expect(store.state.chatQueue).toEqual([]);
  });
});
