// HUP-S1.1 (US-1.1 AC1) and HUP-S1.2 (US-1.4 AC1) — the webview has no tool loop for the local
// model: with Hermes down, local chat is a plain reply with no tools that says so. The one in-app
// loop left (a configured gateway provider, which the sidecar does not serve) offers at most 8
// tool schemas per request and stops after 8 model requests, like the sidecar.
import { describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import {
  AGENT_MAX_TURNS,
  AGENT_TOOLS,
  SIDECAR_DOWN_NOTICE,
  TOOL_SCHEMA_CEILING,
  createAgentProvider,
  createSidecarDownProvider,
  selectToolSchemas,
  type SendOpts,
  type TurnActivityEvent,
} from "./harness";

const ctx = { height: 1, peers: 2, finalityAge: 3, nodeState: "synced", staked: 0, liquid: 0, claimable: 0, earningsToday: 0, walletAddr: "0x0", tier: "T0" };
const src = (p: string) => readFileSync(resolve(__dirname, "..", "..", p), "utf8");

function opts(text: string, onToolCall = vi.fn(async () => "ok")) {
  const activity: TurnActivityEvent[] = [];
  const tokens: string[] = [];
  const o: SendOpts = {
    messages: [{ role: "user", content: text }],
    callbacks: {
      onStatus: () => undefined,
      onToken: (t) => tokens.push(t),
      onToolCall,
      onActivity: (e) => activity.push(e),
    },
  };
  return { o, activity, tokens, onToolCall };
}

describe("US-1.1 AC1: no tool loop for the local model in the webview", () => {
  it("the Hermes-down fallback sends no tools, runs no tool, and says so first", async () => {
    const inferLocal = vi.fn(async () => '{"tool_calls":[{"function":{"name":"group_create"}}]}');
    const p = createSidecarDownProvider(() => ctx, inferLocal);
    const { o, activity, tokens, onToolCall } = opts("make me a group called test");
    const reply = await p.send(o);
    // The local call carries only the messages and the live context: no tool schemas.
    expect(inferLocal).toHaveBeenCalledTimes(1);
    const args = inferLocal.mock.calls[0] as unknown as unknown[];
    expect(args.length).toBe(2);
    expect(JSON.parse(args[0] as string)).toEqual([{ role: "user", content: "make me a group called test" }]);
    // Even a reply that looks like a tool call is just text: nothing runs.
    expect(onToolCall).not.toHaveBeenCalled();
    expect(reply.content).toContain("tool_calls");
    expect(tokens.join("")).toBe(reply.content);
    expect(activity[0]).toEqual({ kind: "notice", text: SIDECAR_DOWN_NOTICE });
    expect(p.kind).toBe("local");
    expect(p.label).toMatch(/no tools/);
  });

  it("the app wires no in-app tool loop to the local model, for chat or for daemons", () => {
    const harness = src("src/agent/harness.ts");
    const store = src("src/shell/store.ts");
    const daemon = src("src/daemons/turn.ts");
    expect(harness).not.toMatch(/export function createLocalAgentProvider/);
    for (const [name, text] of [["store.ts", store], ["daemons/turn.ts", daemon]] as const) {
      expect(text, name).not.toMatch(/createLocalAgentProvider|inferLocalTools/);
    }
    // The local model's agent turns go through the sidecar provider; the fallback is the no-tools one.
    expect(store).toMatch(/createSidecarProvider\(/);
    expect(store).toMatch(/createSidecarDownProvider\(/);
    expect(daemon).toMatch(/createSidecarProvider\(/);
  });
});

describe("US-1.4 AC1: the remaining in-app loop (a gateway provider) stays inside the sidecar's limits", () => {
  it("caps a turn at 8 model requests, like the sidecar's step budget", async () => {
    expect(AGENT_MAX_TURNS).toBe(8);
    const infer = vi.fn(async () => JSON.stringify({ role: "assistant", content: null, tool_calls: [{ id: "a", function: { name: "node_status", arguments: "{}" } }] }));
    const p = createAgentProvider("gw", () => ctx, infer);
    const { o, onToolCall } = opts("loop forever");
    await expect(p.send(o)).rejects.toThrow(/tool-turn limit/);
    expect(infer).toHaveBeenCalledTimes(8);
    expect(onToolCall).toHaveBeenCalledTimes(8);
  });

  it("offers at most 8 tool schemas per request, the relevant ones, keeping the tool in use", async () => {
    expect(TOOL_SCHEMA_CEILING).toBe(8);
    expect(AGENT_TOOLS.length).toBeGreaterThan(TOOL_SCHEMA_CEILING);
    const replies = [
      { role: "assistant", content: null, tool_calls: [{ id: "a", function: { name: "calendar_list", arguments: "{}" } }] },
      { role: "assistant", content: "You have two events." },
    ];
    const infer = vi.fn(async () => JSON.stringify(replies.shift()));
    const p = createAgentProvider("gw", () => ctx, infer);
    await p.send(opts("what is my node status and peers").o);
    expect(infer).toHaveBeenCalledTimes(2);
    for (const c of infer.mock.calls as unknown as [string, string, string, string][]) {
      const offered = JSON.parse(c[2]) as { function: { name: string } }[];
      expect(offered.length).toBeLessThanOrEqual(TOOL_SCHEMA_CEILING);
      expect(offered.length).toBe(TOOL_SCHEMA_CEILING);
    }
    const first = (JSON.parse((infer.mock.calls[0] as unknown as string[])[2]) as { function: { name: string } }[]).map((t) => t.function.name);
    expect(first).toContain("node_status");
    // The tool the model just used stays offered (first) on the next request.
    const second = (JSON.parse((infer.mock.calls[1] as unknown as string[])[2]) as { function: { name: string } }[]).map((t) => t.function.name);
    expect(second[0]).toBe("calendar_list");
    expect(second).toContain("node_status");
  });
});

describe("selectToolSchemas", () => {
  it("ranks by the request's words, keeps catalog order on ties, and never exceeds the ceiling", () => {
    const picked = selectToolSchemas(AGENT_TOOLS, "search my memory for the journal notes").map((t) => t.function.name);
    expect(picked.length).toBe(TOOL_SCHEMA_CEILING);
    expect(picked).toContain("memory_search");
    expect(picked).toContain("journal_read");
    const none = selectToolSchemas(AGENT_TOOLS, "").map((t) => t.function.name);
    expect(none).toEqual(AGENT_TOOLS.slice(0, TOOL_SCHEMA_CEILING).map((t) => t.function.name));
  });

  it("puts tools in use first, most recent first, without duplicates", () => {
    const picked = selectToolSchemas(AGENT_TOOLS, "node status", ["groups_list", "node_status", "groups_list", "not_a_tool"]).map((t) => t.function.name);
    expect(picked.slice(0, 2)).toEqual(["groups_list", "node_status"]);
    expect(new Set(picked).size).toBe(picked.length);
    expect(picked.length).toBe(TOOL_SCHEMA_CEILING);
  });

  it("honours a smaller ceiling", () => {
    expect(selectToolSchemas(AGENT_TOOLS, "anything", [], 3).length).toBe(3);
  });
});
