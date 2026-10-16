// g2-knowledge (b): the QA eval answers the way the app does. The model is offered the app's own
// memory_search tool, the eval runs the call against a mem-mcp daemon holding the imported corpus,
// renders the result with the app's formatter, and loops with the app's turn bound. Red-first.
import { describe, it, expect } from "vitest";
import { AGENT_MAX_TURNS, AGENT_TOOLS } from "../harness";
import { MEMORY_SEARCH_TOOL } from "../knowledgeSearch";
import { QA_TOOL_MAX_TURNS, answerWithMemoryTool, type ChatMessage } from "./toolLoop";

const DOCS = [
  "tenant 'citrate-docs' — 1688 nodes, showing 1:",
  "  0a1b2c3d4e 0.812 [doc] Genesis › What it is",
  "    cite: citrate-docs:content/chain/genesis.md#what-it-is",
  "    > Genesis › What it is",
  "    >",
  "    > The chain id is 40204.",
  "",
].join("\n");

const call = (id: string, args: Record<string, unknown>, name = "memory_search"): ChatMessage => ({
  role: "assistant",
  content: null,
  tool_calls: [{ id, type: "function", function: { name, arguments: JSON.stringify(args) } }],
});

describe("the QA tool loop mirrors the app", () => {
  it("offers exactly the app's memory_search tool and the app's turn bound", () => {
    expect(AGENT_TOOLS).toContain(MEMORY_SEARCH_TOOL);
    expect(QA_TOOL_MAX_TURNS).toBe(AGENT_MAX_TURNS);
  });

  it("runs the model's search on the tenant it picked, feeds back the app rendering, and records node ids", async () => {
    const seen: ChatMessage[][] = [];
    const searches: [string, string, number, boolean][] = [];
    const replies = [call("c1", { query: "chain id", tenant: "citrate-docs" }), { role: "assistant", content: "40204 (citrate-docs:content/chain/genesis.md#what-it-is)" }];
    const out = await answerWithMemoryTool("SYS", "What is the chain id?", {
      complete: async (messages, tools) => {
        seen.push(structuredClone(messages));
        expect(tools).toEqual([MEMORY_SEARCH_TOOL]);
        return replies.shift() as ChatMessage;
      },
      search: async (tenant, query, k, passages) => {
        searches.push([tenant, query, k, passages]);
        return DOCS;
      },
    });
    expect(searches).toEqual([["citrate-docs", "chain id", 5, true]]);
    expect(out.text).toBe("40204 (citrate-docs:content/chain/genesis.md#what-it-is)");
    expect(out.calls).toEqual([{ tenant: "citrate-docs", query: "chain id" }]);
    expect(out.retrieved).toEqual([{ id: "0a1b2c3d4e", cite: "citrate-docs:content/chain/genesis.md#what-it-is" }]);
    expect(out.turnLimit).toBe(false);
    const tool = seen[1].find((m) => m.role === "tool");
    expect(tool?.tool_call_id).toBe("c1");
    expect(tool?.content).toBe("[1] cite as citrate-docs:content/chain/genesis.md#what-it-is\nGenesis › What it is\n\nThe chain id is 40204.");
    expect(seen[0][0]).toEqual({ role: "system", content: "SYS" });
  });

  it("defaults an unknown tenant to citrate-docs, as the app does", async () => {
    const searches: string[] = [];
    const replies = [call("c1", { query: "x", tenant: "bogus" }), { role: "assistant", content: "done" }];
    await answerWithMemoryTool("S", "Q", {
      complete: async () => replies.shift() as ChatMessage,
      search: async (tenant) => (searches.push(tenant), DOCS),
    });
    expect(searches).toEqual(["citrate-docs"]);
  });

  it("answers a tool it does not offer with an error the model can read, and survives bad JSON arguments", async () => {
    const seen: ChatMessage[][] = [];
    const bad: ChatMessage = { role: "assistant", content: null, tool_calls: [{ id: "c2", type: "function", function: { name: "memory_search", arguments: "{not json" } }] };
    const replies = [call("c1", {}, "wallet_send"), bad, { role: "assistant", content: "ok" }];
    const searches: string[] = [];
    const out = await answerWithMemoryTool("S", "Q", {
      complete: async (m) => (seen.push(structuredClone(m)), replies.shift() as ChatMessage),
      search: async (_t, q) => (searches.push(q), DOCS),
    });
    expect(seen[1].at(-1)?.content).toMatch(/not available in this evaluation/);
    expect(searches).toEqual([""]);
    expect(out.text).toBe("ok");
  });

  it("stops at the turn bound with an empty answer and says so", async () => {
    let n = 0;
    const out = await answerWithMemoryTool("S", "Q", {
      complete: async () => call(`c${n++}`, { query: "again" }),
      search: async () => DOCS,
    });
    expect(n).toBe(QA_TOOL_MAX_TURNS);
    expect(out.text).toBe("");
    expect(out.turnLimit).toBe(true);
  });

  it("propagates a daemon failure so the run aborts instead of scoring a fake miss", async () => {
    await expect(
      answerWithMemoryTool("S", "Q", {
        complete: async () => call("c1", { query: "x" }),
        search: async () => {
          throw new Error("cannot reach the memory daemon");
        },
      }),
    ).rejects.toThrow(/memory daemon/);
  });
});
