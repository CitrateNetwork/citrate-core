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
import { createDemoProvider, createRealProvider, createLocalProvider, createAgentProvider, AGENT_MAX_TURNS, type AgentContext, type ChatStatus, type ToolCall } from "./harness";

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

// Q-A.4a item 6 — the local provider activates the already-shipped Rust ai_chat_local.
describe("createLocalProvider — REAL local inference via injected inferLocal (BC-3.2)", () => {
  it("is labeled a LOCAL model, calls inferLocal with ONLY messages+context (no provider/url), and reveals the REAL completion", async () => {
    const inferLocal = vi.fn(async () => "Local model: your node is at height 131234.");
    const provider = createLocalProvider(() => ctx, inferLocal);
    expect(provider.kind).toBe("local");
    expect(provider.label).toContain("local");
    expect(provider.label).not.toContain("built-in demo");

    const cc = collectCallbacks();
    const result = await provider.send({ messages: [{ role: "user", content: "how is my node?" }], callbacks: cc.callbacks });

    expect(inferLocal).toHaveBeenCalledTimes(1);
    // The LOCAL path takes ONLY (messagesJson, contextJson) — NO provider id, NO url
    // (Rust owns the loopback endpoint; the webview can't redirect it).
    const args = inferLocal.mock.calls[0];
    expect(args.length).toBe(2);
    expect(JSON.parse(args[0])).toEqual([{ role: "user", content: "how is my node?" }]);
    expect(JSON.parse(args[1]).height).toBe(131234);

    // Streamed content is EXACTLY the real local completion — never fabricated.
    expect(result.content).toBe("Local model: your node is at height 131234.");
    expect(cc.streamed).toBe("Local model: your node is at height 131234.");
    expect(cc.statuses[cc.statuses.length - 1]).toBe("done");
  });

  it("surfaces a local-server error honestly (onStatus error + throw), never a fabricated reply", async () => {
    const inferLocal = vi.fn(async () => {
      throw new Error("local server not healthy");
    });
    const provider = createLocalProvider(() => ctx, inferLocal);
    const cc = collectCallbacks();
    await expect(provider.send({ messages: [{ role: "user", content: "hi" }], callbacks: cc.callbacks })).rejects.toThrow(/local/);
    expect(cc.statuses).toContain("error");
    expect(cc.streamed).toBe("");
  });
});

// Q-A.4a item 7 — the demo provider's fabricated memory_recall / docs_link replies
// are GONE. In demo mode the agent must NOT invent recalled memory facts nor claim
// it "linked" docs (a no-op). These are the RED-then-GREEN tripwires (Rule 1).
describe("createDemoProvider — no fabricated recall/docs claims (Rule 1)", () => {
  // Drive the demo provider through one prompt and return the fully streamed text.
  async function ask(prompt: string): Promise<{ text: string; toolNames: string[] }> {
    const provider = createDemoProvider(() => ctx);
    let streamed = "";
    const toolNames: string[] = [];
    await provider.send({
      messages: [{ role: "user", content: prompt }],
      callbacks: {
        onStatus: () => {},
        onToken: (t: string) => {
          streamed += t;
        },
        onToolCall: async (c: ToolCall) => {
          toolNames.push(c.name);
          return "ok";
        },
      },
    });
    return { text: streamed, toolNames };
  }

  it("a 'what do you know about me' prompt does NOT invent recalled facts and admits it can't read the graph in demo mode", async () => {
    const { text, toolNames } = await ask("what do you know about me? recall my memory");
    // The old fabricated recall named specific facts that don't exist here.
    expect(text).not.toContain("validator key ceremony completed at onboarding");
    expect(text).not.toContain("gateway key is bound to this device");
    expect(text).not.toMatch(/from your memory graph:/i);
    // It says so honestly instead.
    expect(text.toLowerCase()).toContain("can't read your memory graph in demo mode");
    // And it fires NO memory_recall tool (there is nothing to recall from).
    expect(toolNames).not.toContain("memory_recall");
  });

  it("a 'how do i' docs prompt does NOT falsely claim it linked guides", async () => {
    const { text, toolNames } = await ask("how do i run a validator? show me the guide");
    expect(text).not.toMatch(/I linked the closest Almanac guides/i);
    expect(text.toLowerCase()).toContain("docs linking isn't available in demo mode");
    expect(toolNames).not.toContain("docs_link");
  });

  it("a 'remember this' prompt does NOT claim a durable write occurred on approval (demo writes nothing)", async () => {
    const provider = createDemoProvider(() => ctx);
    let streamed = "";
    await provider.send({
      messages: [{ role: "user", content: "remember that I prefer testnet" }],
      callbacks: {
        onStatus: () => {},
        onToken: (t: string) => {
          streamed += t;
        },
        // The ceremony "approved" — but the demo agent still must not claim the
        // fact "now lives" in the graph (it isn't connected to the daemon).
        onToolCall: async () => "approved",
      },
    });
    expect(streamed).not.toMatch(/it now lives in your personal tenant/i);
    expect(streamed.toLowerCase()).toContain("nothing was durably stored");
  });
});

// W3.3 — the agentic tool loop: the model decides tool calls; the frontend
// executes them via onToolCall, feeds results back, and streams the final answer.
describe("createAgentProvider — real tool loop (W3.3)", () => {
  function toolCallMsg(name: string, args: object): string {
    return JSON.stringify({
      role: "assistant",
      content: null,
      tool_calls: [{ id: "c1", type: "function", function: { name, arguments: JSON.stringify(args) } }],
    });
  }
  function contentMsg(content: string): string {
    return JSON.stringify({ role: "assistant", content });
  }

  it("executes a tool call, feeds the result back, then streams the final answer", async () => {
    // Turn 1: model asks for memory_search. Turn 2: model answers.
    const inferTools = vi
      .fn()
      .mockResolvedValueOnce(toolCallMsg("memory_search", { query: "staking" }))
      .mockResolvedValueOnce(contentMsg("Staking locks 32,000 SALT (from the docs)."));
    const toolCalls: ToolCall[] = [];
    const statuses: ChatStatus[] = [];
    let streamed = "";
    const provider = createAgentProvider("gateway", () => ctx, inferTools);
    expect(provider.kind).toBe("agent");

    const result = await provider.send({
      messages: [{ role: "user", content: "how does staking work?" }],
      callbacks: {
        onStatus: (s) => statuses.push(s),
        onToken: (t) => {
          streamed += t;
        },
        onToolCall: async (c) => {
          toolCalls.push(c);
          return "[1] Staking › lock 32000 SALT";
        },
      },
    });

    // The tool was executed with the model's chosen name + args.
    expect(toolCalls).toHaveLength(1);
    expect(toolCalls[0].name).toBe("memory_search");
    expect(JSON.parse(toolCalls[0].arguments).query).toBe("staking");

    // Two model turns; the SECOND request carried the tool result back.
    expect(inferTools).toHaveBeenCalledTimes(2);
    const secondConvo = JSON.parse(inferTools.mock.calls[1][1]);
    const toolMsg = secondConvo.find((m: { role: string }) => m.role === "tool");
    expect(toolMsg.tool_call_id).toBe("c1");
    expect(toolMsg.content).toContain("32000 SALT");
    // The tools spec + context were passed on every turn.
    expect(JSON.parse(inferTools.mock.calls[0][2]).length).toBeGreaterThan(0);
    expect(JSON.parse(inferTools.mock.calls[0][3]).nodeState).toBe("validating");

    // The final answer is the REAL streamed content, ending in done.
    expect(result.content).toBe("Staking locks 32,000 SALT (from the docs).");
    expect(streamed).toBe("Staking locks 32,000 SALT (from the docs).");
    expect(statuses).toContain("tool");
    expect(statuses[statuses.length - 1]).toBe("done");
  });

  it("is bounded: a model that never stops calling tools fails honestly, not forever", async () => {
    const inferTools = vi.fn(async () => toolCallMsg("memory_search", { query: "x" }));
    const provider = createAgentProvider("gateway", () => ctx, inferTools);
    const statuses: ChatStatus[] = [];
    await expect(
      provider.send({
        messages: [{ role: "user", content: "loop" }],
        callbacks: { onStatus: (s) => statuses.push(s), onToken: () => {}, onToolCall: async () => "again" },
      }),
    ).rejects.toThrow(/tool-turn limit/);
    expect(inferTools).toHaveBeenCalledTimes(AGENT_MAX_TURNS);
    expect(statuses).toContain("error");
  });

  it("surfaces a provider error honestly (no fabricated reply)", async () => {
    const inferTools = vi.fn(async () => {
      throw new Error("ai: gateway error");
    });
    const provider = createAgentProvider("gateway", () => ctx, inferTools);
    const statuses: ChatStatus[] = [];
    let streamed = "";
    await expect(
      provider.send({
        messages: [{ role: "user", content: "hi" }],
        callbacks: { onStatus: (s) => statuses.push(s), onToken: (t) => (streamed += t), onToolCall: async () => "x" },
      }),
    ).rejects.toThrow(/gateway/);
    expect(statuses).toContain("error");
    expect(streamed).toBe("");
  });
});
