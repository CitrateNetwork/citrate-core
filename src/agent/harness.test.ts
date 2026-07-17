// CORE-AI1 (@rule8) — the chat provider harness. Covers:
//   * the demo provider's HONEST label (Rule 1: "built-in demo agent", never a
//     gateway/local-proxy it does not call);
//   * createRealProvider: it calls the injected `infer` (→ Rust ai_chat) with the
//     provider id + serialized messages + live context, and reveals the REAL
//     completion via onToken (a display animation over the real content, no tool
//     loop, never fabricated);
//   * a provider-error surfaces honestly (onStatus("error") + throw), never a
//     fabricated reply.
import { describe, it, expect, vi } from "vitest";
import { createDemoProvider, createRealProvider, type AgentContext, type ChatStatus, type ToolCall } from "./harness";

const ctx: AgentContext = {
  height: 131234,
  peers: 23,
  finalityAge: 8,
  nodeState: "validating",
  staked: 32000,
  liquid: 12.5,
  claimable: 9.41,
  earningsToday: 3.86,
  walletAddr: "0xabc",
  tier: "pilot",
};

function collectCallbacks() {
  const statuses: ChatStatus[] = [];
  let streamed = "";
  return {
    statuses,
    get streamed() {
      return streamed;
    },
    callbacks: {
      onStatus: (st: ChatStatus) => statuses.push(st),
      onToken: (t: string) => {
        streamed += t;
      },
      onToolCall: async (_c: ToolCall) => "unused",
    },
  };
}

describe("createDemoProvider — honest label (Rule 1)", () => {
  it("is labeled a built-in demo agent, never a gateway/local-proxy", () => {
    const p = createDemoProvider(() => ctx);
    expect(p.kind).toBe("demo");
    expect(p.label).toBe("built-in demo agent");
    expect(p.label).not.toContain("infer.citrate.ai");
    expect(p.label).not.toContain("local-proxy");
  });
});

describe("createRealProvider — real inference via injected infer (AI1)", () => {
  it("calls infer with the provider id + serialized messages + live context, and reveals the REAL completion", async () => {
    const infer = vi.fn(async () => "Your node is validating at height 131234.");
    const provider = createRealProvider("gateway", () => ctx, infer);
    expect(provider.kind).toBe("real");

    const cc = collectCallbacks();
    const messages = [{ role: "user", content: "how is my node?" }];
    const result = await provider.send({ messages, callbacks: cc.callbacks });

    // It routed to the injected infer (→ Rust ai_chat) with the RIGHT args.
    expect(infer).toHaveBeenCalledTimes(1);
    const [pid, messagesJson, contextJson] = infer.mock.calls[0];
    expect(pid).toBe("gateway");
    expect(JSON.parse(messagesJson)).toEqual([{ role: "user", content: "how is my node?" }]);
    // The live context is passed through as a JSON snapshot (Rust injects it).
    expect(JSON.parse(contextJson).height).toBe(131234);
    expect(JSON.parse(contextJson).nodeState).toBe("validating");

    // The returned + streamed content is EXACTLY the provider's real completion
    // (revealed via onToken) — never fabricated, no tool loop.
    expect(result.content).toBe("Your node is validating at height 131234.");
    expect(cc.streamed).toBe("Your node is validating at height 131234.");
    expect(cc.statuses).toContain("thinking");
    expect(cc.statuses).toContain("streaming");
    expect(cc.statuses[cc.statuses.length - 1]).toBe("done");
  });

  it("surfaces a provider error honestly (onStatus error + throw), never a fabricated reply", async () => {
    const infer = vi.fn(async () => {
      throw new Error("ai: provider returned an error");
    });
    const provider = createRealProvider("openai", () => ctx, infer);
    const cc = collectCallbacks();
    await expect(provider.send({ messages: [{ role: "user", content: "hi" }], callbacks: cc.callbacks })).rejects.toThrow(
      /provider/,
    );
    expect(cc.statuses).toContain("error");
    // No fabricated content was streamed.
    expect(cc.streamed).toBe("");
  });
});
