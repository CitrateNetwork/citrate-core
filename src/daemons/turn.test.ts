// HUP-S10.3 — one daemon turn: local model only, no UI moves, and every effectful tool call goes to
// the member as an explicit (HIC-required) decision. Sidecar runs use an UNATTENDED session that is
// closed afterwards.
import { describe, it, expect, vi } from "vitest";
import { daemonAvailability, daemonToolDecision, DAEMON_TOOLS, runDaemonTurn, type DaemonTurnDeps } from "./turn";
import { TokenMeter } from "./tokenMeter";
import { AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall, type ToolCallMeta } from "../agent/harness";
import type { Claim } from "./api";

const claim: Claim = { daemonId: "d1", runId: "r1", name: "Node digest", prompt: "Summarise my node.", tokensAllowed: 100_000, startedMs: 0 };
const ctx = { height: 1, peers: 2, finalityAge: 3, nodeState: "synced", staked: 0, liquid: 0, claimable: 0, earningsToday: 0, walletAddr: "0x0", tier: "T0" };
const call = (name: string): ToolCall => ({ id: "c1", name, arguments: "{}" });

function deps(over: Partial<DaemonTurnDeps> = {}): DaemonTurnDeps & { handled: [string, ToolCallMeta | undefined][] } {
  const handled: [string, ToolCallMeta | undefined][] = [];
  return {
    handled,
    providerKind: "local",
    systemPrompt: () => "You are Hermes.",
    context: () => ctx,
    inferLocalTools: vi.fn(async () => JSON.stringify({ role: "assistant", content: "done" })),
    sidecar: null,
    handleTool: async (c, meta) => {
      handled.push([c.name, meta]);
      return "ok";
    },
    ...over,
  };
}

describe("where daemons may run", () => {
  it("only on the local model, in the desktop app", () => {
    expect(daemonAvailability("local", "tauri")).toEqual({ ok: true });
    expect(daemonAvailability("sidecar", "tauri")).toEqual({ ok: true });
    for (const kind of ["agent", "real", "demo", undefined]) {
      const a = daemonAvailability(kind, "tauri");
      expect(a.ok).toBe(false);
    }
    expect(daemonAvailability("local", "sim").ok).toBe(false);
  });
});

describe("daemon tool rules", () => {
  it("offers every agent tool except moving the UI", () => {
    const names = DAEMON_TOOLS.map((t) => t.function.name);
    expect(names).not.toContain("app_navigate");
    expect(names.length).toBe(AGENT_TOOLS.length - 1);
  });

  it("refuses app_navigate and unknown tools without running anything", () => {
    for (const name of ["app_navigate", "sign_approve", "invoke"]) {
      const d = daemonToolDecision(call(name), "Node digest");
      expect(d.run).toBe(false);
    }
  });

  it("runs read-only tools as they are, and marks every other tool HIC-required", () => {
    for (const t of AGENT_TOOLS) {
      const name = t.function.name;
      if (name === "app_navigate") continue;
      const d = daemonToolDecision(call(name), "Node digest");
      expect(d.run).toBe(true);
      if (!d.run) continue;
      if (READ_ONLY_AGENT_TOOLS.has(name)) expect(d.meta).toBeUndefined();
      else {
        expect(d.meta?.hic).toBe("required");
        expect(d.meta?.hicReason).toContain("Node digest");
      }
    }
  });

  it("keeps the sidecar's HIC mark on a read-only call too", () => {
    const d = daemonToolDecision(call("node_status"), "x", { hic: "required", hicReason: "tainted" });
    expect(d.run && d.meta?.hic).toBe("required");
  });
});

describe("a local daemon turn", () => {
  it("sends the task, runs tools through the gated handler, meters it and returns the reply", async () => {
    const replies = [
      { role: "assistant", content: null, tool_calls: [{ id: "a", function: { name: "node_status", arguments: "{}" } }, { id: "b", function: { name: "journal_append", arguments: "{\"entry\":\"x\"}" } }] },
      { role: "assistant", content: "Node is synced." },
    ];
    const infer = vi.fn(async () => JSON.stringify(replies.shift()));
    const d = deps({ inferLocalTools: infer });
    const meter = new TokenMeter(100_000, () => undefined);
    const out = await runDaemonTurn(claim, new AbortController().signal, meter, d);
    expect(out).toBe("Node is synced.");
    // The daemon's tool list (no app_navigate) is what the model saw.
    const firstCall = infer.mock.calls[0] as unknown as [string, string, string];
    expect(firstCall[1]).toBe(JSON.stringify(DAEMON_TOOLS));
    expect(firstCall[0]).toContain("Summarise my node.");
    expect(d.handled.map(([n, m]) => [n, m?.hic ?? null])).toEqual([["node_status", null], ["journal_append", "required"]]);
    expect(meter.tokens()).toBeGreaterThan(0);
  });

  it("refuses to start on a provider that is not the local model", async () => {
    await expect(runDaemonTurn(claim, new AbortController().signal, new TokenMeter(1, () => undefined), deps({ providerKind: "agent" }))).rejects.toThrow(/local model/);
  });

  it("a tool call after the run was stopped is not run", async () => {
    const ac = new AbortController();
    const d = deps({
      inferLocalTools: vi.fn(async () => {
        ac.abort();
        return JSON.stringify({ role: "assistant", content: null, tool_calls: [{ id: "a", function: { name: "journal_append", arguments: "{}" } }] });
      }),
    });
    await expect(runDaemonTurn(claim, ac.signal, new TokenMeter(100_000, () => undefined), d)).rejects.toThrow();
    expect(d.handled).toEqual([]);
  });
});

describe("a sidecar daemon turn", () => {
  it("opens an unattended session, forwards the HIC mark, and closes the session", async () => {
    const pages = [
      { events: [{ seq: 1, event: { type: "step_start", step: 1 } }, { seq: 2, event: { type: "tool_call", host: "core", hic: "required", hic_reason: "unattended", call: { id: "c1", name: "journal_append", arguments: "{}" } } }], lastSeq: 2, busy: true },
      { events: [{ seq: 3, event: { type: "tool_result", call_id: "c1" } }, { seq: 4, event: { type: "final", content: "Wrote nothing; asked you." } }, { seq: 5, event: { type: "done", outcome: "answered" } }], lastSeq: 5, busy: false },
    ];
    const sidecar = {
      openUnattended: vi.fn(async () => "s1-ab"),
      send: vi.fn(async () => undefined),
      events: vi.fn(async () => pages.shift() ?? { events: [], lastSeq: 5, busy: false }),
      toolResult: vi.fn(async () => undefined),
      stop: vi.fn(async () => undefined),
      close: vi.fn(async () => undefined),
    };
    const d = deps({ providerKind: "sidecar", sidecar });
    const out = await runDaemonTurn(claim, new AbortController().signal, new TokenMeter(100_000, () => undefined), d);
    expect(out).toBe("Wrote nothing; asked you.");
    expect(sidecar.openUnattended).toHaveBeenCalledTimes(1);
    expect(JSON.parse((sidecar.openUnattended.mock.calls[0] as unknown as [string, string])[1]).map((t: { function: { name: string } }) => t.function.name)).not.toContain("app_navigate");
    expect(d.handled).toEqual([["journal_append", { hic: "required", hicReason: expect.stringContaining("Node digest") }]]);
    expect(sidecar.close).toHaveBeenCalledWith("s1-ab");
  });
});
