// @vitest-environment node
// HUP-S1.1 proof (g1-loop, g1-render) — the UI provider against a REAL Hermes sidecar binary and a
// real llama-server. Skipped unless HERMES_LIVE_ADDR (sidecar control address), HERMES_LIVE_TOKEN_FILE
// (its bearer file) and HERMES_LIVE_LLM (an OpenAI-compatible base URL) are set. Writes a transcript
// (every event with its seq, which provider saw it, and the session id) to HERMES_LIVE_OUT.
//
// The one core-hosted tool, `chain_head`, is answered by this test from the public 40204 RPC
// (eth_blockNumber), the same source the node MCP server's chain_head falls back to.
import { describe, it, expect } from "vitest";
import { writeFileSync } from "node:fs";
import { readFileSync } from "node:fs";
import { createSidecarProvider, type SavedSidecarSession, type SidecarSessionApi } from "./sidecarProvider";
import type { ToolCall } from "./harness";

const ADDR = process.env.HERMES_LIVE_ADDR ?? "";
const TOKEN_FILE = process.env.HERMES_LIVE_TOKEN_FILE ?? "";
const LLM = process.env.HERMES_LIVE_LLM ?? "";
const MODEL = process.env.HERMES_LIVE_MODEL ?? "gemma-4-E4B-it-Q4_0.gguf";
const OUT = process.env.HERMES_LIVE_OUT ?? "";
const RPC = "https://rpc.citrate.ai";

type Page = { events: { seq: number; event: Record<string, unknown> }[]; lastSeq: number; busy: boolean; pendingCoreCalls?: string[] };

function liveApi(log: { who: string; seq: number; event: Record<string, unknown> }[], who: () => string): SidecarSessionApi {
  const bearer = ADDR ? readFileSync(TOKEN_FILE, "utf8").trim() : "";
  const call = async (method: string, path: string, body?: unknown) => {
    const r = await fetch(`http://${ADDR}${path}`, {
      method,
      headers: { authorization: `Bearer ${bearer}`, "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await r.text();
    if (!r.ok) throw new Error(`hermes control returned ${r.status}: ${text.slice(0, 200)}`);
    return text ? JSON.parse(text) : null;
  };
  return {
    open: async (systemPrompt, toolsJson) => {
      const v = await call("POST", "/sessions", {
        model: MODEL,
        systemPrompt,
        llm: { baseUrl: LLM, bearer: "" },
        tools: JSON.parse(toolsJson),
        maxToolsPerRequest: 8,
        maxSteps: 6,
        hicAware: true,
      });
      return String(v.id);
    },
    send: async (id, text) => {
      await call("POST", `/sessions/${id}/messages`, { text });
    },
    events: async (id, after, waitMs) => {
      const page = (await call("GET", `/sessions/${id}/events?after=${after}&wait_ms=${Math.min(waitMs, 5000)}`)) as Page;
      for (const e of page.events) if (e.seq > after && !log.some((l) => l.seq === e.seq && l.who === who())) log.push({ who: who(), seq: e.seq, event: e.event });
      return page;
    },
    position: async (id) => ((await call("GET", `/sessions/${id}/events?after=${Number.MAX_SAFE_INTEGER}&wait_ms=0`)) as Page).lastSeq,
    toolResult: async (id, callId, status, content) => {
      await call("POST", `/sessions/${id}/tool_results`, { callId, status, content });
    },
    stop: async (id) => {
      await call("POST", `/sessions/${id}/stop`, {});
    },
  };
}

async function chainHead(): Promise<string> {
  const r = await fetch(RPC, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "eth_blockNumber", params: [] }) });
  const v = (await r.json()) as { result?: string };
  if (!v.result) throw new Error("the public RPC returned no block number");
  return JSON.stringify({ chainId: 40204, height: parseInt(v.result, 16), source: "public-rpc" });
}

const TOOLS = [
  {
    name: "chain_head",
    description: "The Citrate chain id and current head block height.",
    parameters: { type: "object", properties: {}, additionalProperties: false },
    host: "core",
    annotations: { effect: "none", trust: "trusted" },
  },
];

const SYSTEM = "You are Hermes, the member's Citrate agent. When asked about the chain height, call chain_head first, then answer in one short sentence.";

describe.skipIf(!ADDR || !TOKEN_FILE || !LLM)("HUP-S1.1 live: the UI provider over a real sidecar", () => {
  it(
    "streams, survives a view teardown mid-turn, and the reloaded view finishes the turn from the saved seq",
    async () => {
      const log: { who: string; seq: number; event: Record<string, unknown> }[] = [];
      let current = "view-A";
      const api = liveApi(log, () => current);
      let saved: SavedSidecarSession | null = null;
      const store = { load: () => saved, save: (v: SavedSidecarSession | null) => (saved = v) };
      const notes: string[] = [];

      // Turn 1 in view A: answered normally, streamed.
      let textA = "";
      const viewA = createSidecarProvider(api, () => SYSTEM, () => TOOLS, () => null, { store });
      const ran: string[] = [];
      const out1 = await viewA.send({
        messages: [{ role: "user", content: "What is the current Citrate chain height?" }],
        callbacks: {
          onStatus: () => undefined,
          onToken: (t) => (textA += t),
          onToolCall: async (c: ToolCall) => {
            ran.push(`A:${c.name}`);
            return chainHead();
          },
          onActivity: (ev) => ev.kind === "notice" && notes.push(ev.text),
        },
      });
      expect(textA).toBe(out1.content);
      const sessionId = saved!.id;

      // Turn 2 in view A: the view goes away while the member's tool call is in progress.
      let reachedTool = false;
      void viewA
        .send({
          messages: [{ role: "user", content: "Check the chain height again, please." }],
          callbacks: {
            onStatus: () => undefined,
            onToken: () => undefined,
            onToolCall: async (c: ToolCall) => {
              ran.push(`A:${c.name}`);
              reachedTool = true;
              return new Promise<string>(() => {});
            },
          },
        })
        .catch(() => undefined);
      for (let i = 0; i < 600 && !reachedTool; i++) await new Promise((r) => setTimeout(r, 100));
      expect(reachedTool).toBe(true);
      const savedAtTeardown = JSON.parse(JSON.stringify(saved));

      // View B: a new provider over the saved state (the webview reloaded).
      current = "view-B";
      let textB = "";
      const viewB = createSidecarProvider(api, () => SYSTEM, () => TOOLS, () => null, { store });
      const r = await viewB.reattach!({
        callbacks: {
          onStatus: () => undefined,
          onToken: (t) => (textB += t),
          onToolCall: async (c: ToolCall) => {
            ran.push(`B:${c.name}`);
            return chainHead();
          },
          onActivity: (ev) => ev.kind === "notice" && notes.push(ev.text),
        },
      });
      expect(r.kind).toBe("resumed");
      const seqsB = log.filter((l) => l.who === "view-B").map((l) => l.seq);
      expect(Math.min(...seqsB)).toBeGreaterThan(savedAtTeardown.lastSeq);
      const all = log.map((l) => l.seq);
      expect(new Set(all).size).toBe(all.length);

      if (OUT) {
        writeFileSync(
          OUT,
          JSON.stringify(
            {
              sessionId,
              model: MODEL,
              turn1: { answer: out1.content, streamedText: textA },
              savedAtTeardown,
              reattach: r,
              viewBText: textB,
              toolRuns: ran,
              notices: notes,
              events: log,
            },
            null,
            2,
          ),
        );
      }
    },
    600_000,
  );
});
