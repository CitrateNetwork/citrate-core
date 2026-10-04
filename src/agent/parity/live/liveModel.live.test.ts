// @vitest-environment node
// HUP-S1.9: the sidecar loop on a REAL local model. Core's sidecar provider drives the packaged
// sidecar, whose session talks to a running llama-server (the packaged one, with the T0 model), for
// a handful of everyday member asks. Core-hosted tools answer with fixed read results, so what
// varies is only the model. This is a behaviour record (steps used, tools chosen, honest endings),
// not an equality check against harness.ts: a real model is not deterministic.
//
// Runs only with CITRATE_PARITY_LIVE_SIDECAR (the sidecar binary) AND CITRATE_PARITY_LIVE_LLM (the
// llama-server `.../v1` base URL); CITRATE_PARITY_LIVE_LLM_KEY_FILE names a file holding its API
// key, CITRATE_PARITY_LIVE_LLM_CTX its --ctx-size, CITRATE_PARITY_LIVE_MODEL_REPORT the report path.
import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { annotatedAgentTools } from "../../toolAnnotations";
import { AGENT_SYSTEM_PROMPT, type ToolCall } from "../../harness";
import { createSidecarProvider } from "../../sidecarProvider";
import { startSidecar, HttpSessionApi } from "../../../../scripts/parity-live/sidecar-http.mjs";
import { buildSessionBody, liveStepCap, SESSION_CONFIG, type RecordingSessionApi } from "./liveRunner";

const BIN = process.env.CITRATE_PARITY_LIVE_SIDECAR ?? "";
const LLM = process.env.CITRATE_PARITY_LIVE_LLM ?? "";
const KEY_FILE = process.env.CITRATE_PARITY_LIVE_LLM_KEY_FILE ?? "";
const CTX = Number(process.env.CITRATE_PARITY_LIVE_LLM_CTX ?? "16384");
const REPORT = process.env.CITRATE_PARITY_LIVE_MODEL_REPORT ?? "";
const live = BIN !== "" && existsSync(BIN) && LLM !== "";

// Fixed results for the app's read tools (shaped like the store's handlers' replies), and an
// approval for a write: only the model's choices vary between runs.
const READS: Record<string, string> = {
  node_status: JSON.stringify({ nodeState: "synced", height: 71234, peers: 5, finalityAge: 3 }),
  staking_status: JSON.stringify({ staked: 32000, liquid: 1.25, claimable: 0.4, address: "0x0000000000000000000000000000000000000001" }),
  memory_search: "[1] Validator keys stay in this device's keychain; Hermes never holds a key. (citrate-docs: keys-and-the-ceremony)",
  memory_recall: "No saved facts match.",
  groups_list: JSON.stringify([{ name: "Builders", members: 4 }]),
  journal_read: "No journal entries today.",
  skills_list: JSON.stringify({ onChain: [], local: [] }),
  models_list: JSON.stringify({ local: ["gemma-4-E4B-it-Q4_0.gguf"], onChain: [] }),
};
const WRITE_APPROVED = "Saved: the member approved this change.";

const ASKS: { id: string; text: string; expectTool?: string }[] = [
  { id: "greeting", text: "Hi! In two sentences, what can you help me with?" },
  { id: "node", text: "Is my node synced, and how many peers do I have?", expectTool: "node_status" },
  { id: "stake", text: "How much SALT do I have staked right now?", expectTool: "staking_status" },
  { id: "docs", text: "Search the docs: who holds my validator keys?", expectTool: "memory_search" },
  { id: "groups", text: "Which groups am I in?", expectTool: "groups_list" },
  { id: "remember", text: "Please remember that my validator is named alpha.", expectTool: "memory_assert" },
];

type Row = { id: string; outcome: string | null; steps: number; tools: string[]; expectTool: string | null; usedExpected: boolean | null; seconds: number; answer: string; error: string | null };

describe.skipIf(!live)("sidecar loop on a real local model (HUP-S1.9 live)", () => {
  let sidecar: { baseUrl: string; bearer: string; stderr: () => string; stop: () => Promise<void> };
  const rows: Row[] = [];
  const llmKey = KEY_FILE ? readFileSync(KEY_FILE, "utf8").trim() : "";

  beforeAll(async () => {
    sidecar = await startSidecar(BIN, {});
  }, 60_000);

  afterAll(async () => {
    await sidecar?.stop();
    if (REPORT) writeFileSync(REPORT, JSON.stringify({ llm: LLM, ctx: CTX, step_cap: liveStepCap(), rows }, null, 2));
  });

  for (const ask of ASKS) {
    it(ask.id, async () => {
      const api = new HttpSessionApi(sidecar.baseUrl, sidecar.bearer, (p: string, t: string) =>
        buildSessionBody(SESSION_CONFIG, p, t, { baseUrl: LLM, bearer: llmKey }, "local", CTX),
      ) as RecordingSessionApi;
      const context = { height: 71234, peers: 5, finalityAge: 3, nodeState: "synced", staked: 32000, liquid: 1.25, claimable: 0.4, earningsToday: 0, tier: "member" };
      const provider = createSidecarProvider(
        api,
        () => AGENT_SYSTEM_PROMPT + "\n\nLive app context (JSON snapshot at session start): " + JSON.stringify(context),
        () => annotatedAgentTools(),
      );
      const tools: string[] = [];
      const onToolCall = async (c: ToolCall): Promise<string> => {
        tools.push(c.name);
        return READS[c.name] ?? WRITE_APPROVED;
      };
      const t0 = Date.now();
      let answer = "";
      let error: string | null = null;
      try {
        answer = (await provider.send({ messages: [{ role: "user", content: ask.text }], callbacks: { onStatus: () => undefined, onToken: () => undefined, onToolCall } })).content;
      } catch (e) {
        error = e instanceof Error ? e.message : String(e);
      }
      const id = api.opened[api.opened.length - 1];
      if (id) await api.close(id).catch(() => undefined);
      const done = api.events_seen.find((e) => e.event.type === "done");
      const steps = api.events_seen.filter((e) => e.event.type === "step_start").length;
      const row: Row = {
        id: ask.id,
        outcome: done ? String(done.event.outcome) : null,
        steps,
        tools,
        expectTool: ask.expectTool ?? null,
        usedExpected: ask.expectTool ? tools.includes(ask.expectTool) : null,
        seconds: Math.round((Date.now() - t0) / 100) / 10,
        answer: answer.slice(0, 300),
        error,
      };
      rows.push(row);
      // What must hold whatever the model does: the turn ends in a `done` event, within the cap, and
      // an answered turn returns text; a failed one says why.
      expect(row.outcome, sidecar.stderr().slice(-1500)).not.toBeNull();
      expect(steps).toBeLessThanOrEqual(liveStepCap());
      if (row.outcome === "answered") expect(answer.length).toBeGreaterThan(0);
      else expect(error).not.toBeNull();
    }, 600_000);
  }
});

describe.skipIf(live)("sidecar loop on a real local model (skipped)", () => {
  it("needs CITRATE_PARITY_LIVE_SIDECAR and CITRATE_PARITY_LIVE_LLM; see scripts/parity-live.sh", () => {
    expect(live).toBe(false);
  });
});
