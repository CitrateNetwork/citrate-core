// HUP-S10.3 — one daemon turn: local model only, no UI moves, and every effectful tool call goes to
// the member as an explicit (HIC-required) decision. Sidecar runs use an UNATTENDED session that is
// closed afterwards.
import { describe, it, expect, vi } from "vitest";
import { DAEMON_NEEDS_HERMES, daemonAvailability, daemonToolDecision, daemonToolsFor, DAEMON_TOOLS, runDaemonTurn, type DaemonTurnDeps } from "./turn";
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
    sidecar: null,
    handleTool: async (c, meta) => {
      handled.push([c.name, meta]);
      return "ok";
    },
    ...over,
  };
}

describe("where daemons may run", () => {
  it("only on the local model in Hermes, in the desktop app", () => {
    // HUP-S1.1: the local model without the Hermes sidecar has no tool loop, so daemons wait.
    expect(daemonAvailability("local", "tauri")).toEqual({ ok: false, why: DAEMON_NEEDS_HERMES });
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

function sidecarFake(pages: { events: { seq: number; event: Record<string, unknown> }[]; lastSeq: number; busy: boolean }[], onEvents?: () => void) {
  return {
    openUnattended: vi.fn(async () => "s1-ab"),
    send: vi.fn(async () => undefined),
    events: vi.fn(async () => {
      onEvents?.();
      return pages.shift() ?? { events: [], lastSeq: 9, busy: false };
    }),
    toolResult: vi.fn(async () => undefined),
    stop: vi.fn(async () => undefined),
    close: vi.fn(async () => undefined),
  };
}

describe("a daemon turn without Hermes", () => {
  it("refuses on the local model alone and runs nothing (no in-app tool loop)", async () => {
    const d = deps({ providerKind: "local" });
    await expect(runDaemonTurn(claim, new AbortController().signal, new TokenMeter(100_000, () => undefined), d)).rejects.toThrow(DAEMON_NEEDS_HERMES);
    expect(d.handled).toEqual([]);
  });

  it("refuses to start on a provider that is not the local model", async () => {
    await expect(runDaemonTurn(claim, new AbortController().signal, new TokenMeter(1, () => undefined), deps({ providerKind: "agent" }))).rejects.toThrow(/local model/);
  });

  it("a tool call after the run was stopped is not run", async () => {
    const ac = new AbortController();
    const sidecar = sidecarFake(
      [{ events: [{ seq: 1, event: { type: "tool_call", host: "core", call: { id: "a", name: "journal_append", arguments: "{}" } } }], lastSeq: 1, busy: true }],
      () => ac.abort(),
    );
    const d = deps({ providerKind: "sidecar", sidecar });
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
    const sidecar = sidecarFake(pages);
    const d = deps({ providerKind: "sidecar", sidecar });
    const out = await runDaemonTurn(claim, new AbortController().signal, new TokenMeter(100_000, () => undefined), d);
    expect(out).toBe("Wrote nothing; asked you.");
    expect(sidecar.openUnattended).toHaveBeenCalledTimes(1);
    expect(JSON.parse((sidecar.openUnattended.mock.calls[0] as unknown as [string, string])[1]).map((t: { function: { name: string } }) => t.function.name)).not.toContain("app_navigate");
    expect(d.handled).toEqual([["journal_append", { hic: "required", hicReason: expect.stringContaining("Node digest") }]]);
    expect(sidecar.close).toHaveBeenCalledWith("s1-ab");
  });
});

describe("a Store daemon's approved permissions bind its tools (deny by default)", () => {
  const pages = (name: string) => [
    { events: [{ seq: 1, event: { type: "step_start", step: 1 } }, { seq: 2, event: { type: "tool_call", host: "core", call: { id: "c1", name, arguments: "{}" } } }], lastSeq: 2, busy: true },
    { events: [{ seq: 3, event: { type: "tool_result", call_id: "c1" } }, { seq: 4, event: { type: "final", content: "ok" } }, { seq: 5, event: { type: "done", outcome: "answered" } }], lastSeq: 5, busy: false },
  ];
  const offered = (sc: ReturnType<typeof sidecarFake>) =>
    JSON.parse((sc.openUnattended.mock.calls[0] as unknown as [string, string])[1]).map((t: { function: { name: string } }) => t.function.name);

  it("a daemon approved with no tools (no permissions, or only local_model) is offered zero tools and runs none", async () => {
    for (const name of ["memory_recall", "journal_read", "node_status"]) {
      const sidecar = sidecarFake(pages(name));
      const d = deps({ providerKind: "sidecar", sidecar });
      await runDaemonTurn({ ...claim, allowedTools: [] }, new AbortController().signal, new TokenMeter(100_000, () => undefined), d);
      expect(offered(sidecar)).toEqual([]);
      expect(d.handled).toEqual([]);
      expect(sidecar.toolResult).toHaveBeenCalledWith("s1-ab", "c1", expect.anything(), expect.stringContaining("not available"));
    }
  });

  it("a daemon approved for memory reads gets only those: no journal tools", async () => {
    const allowed = ["memory_recall", "memory_search"];
    const sidecar = sidecarFake(pages("journal_read"));
    const d = deps({ providerKind: "sidecar", sidecar });
    await runDaemonTurn({ ...claim, allowedTools: allowed }, new AbortController().signal, new TokenMeter(100_000, () => undefined), d);
    expect([...offered(sidecar)].sort()).toEqual(allowed);
    expect(d.handled).toEqual([]);
    const ok = sidecarFake(pages("memory_recall"));
    const d2 = deps({ providerKind: "sidecar", sidecar: ok });
    await runDaemonTurn({ ...claim, allowedTools: allowed }, new AbortController().signal, new TokenMeter(100_000, () => undefined), d2);
    expect(d2.handled.map(([n]) => n)).toEqual(["memory_recall"]);
  });

  it("the decision refuses any tool outside the approved list, even a read-only one", () => {
    expect(daemonToolDecision(call("memory_recall"), "x", undefined, []).run).toBe(false);
    expect(daemonToolDecision(call("journal_read"), "x", undefined, ["memory_recall"]).run).toBe(false);
    expect(daemonToolDecision(call("memory_recall"), "x", undefined, ["memory_recall"]).run).toBe(true);
    // app_navigate stays refused even if a list names it.
    expect(daemonToolDecision(call("app_navigate"), "x", undefined, ["app_navigate"]).run).toBe(false);
  });

  it("a daemon the member wrote (no approved list) keeps the member-daemon tool set", () => {
    expect(daemonToolsFor(null).length).toBe(DAEMON_TOOLS.length);
    expect(daemonToolsFor([]).length).toBe(0);
  });
});

describe("a Store daemon's prompt carries no app snapshot", () => {
  it("a Store daemon run's system prompt has no live app context; a member daemon's keeps it", async () => {
    const done = () => [
      { events: [{ seq: 1, event: { type: "final", content: "ok" } }, { seq: 2, event: { type: "done", outcome: "answered" } }], lastSeq: 2, busy: false },
    ];
    const marked = { ...ctx, walletAddr: "0xfeed5eed000000000000000000000000000000aa" };
    const snapshotPrompt = () => "You are Hermes.\n\nLive app context (JSON snapshot at session start): " + JSON.stringify(marked);
    const store = sidecarFake(done());
    await runDaemonTurn({ ...claim, allowedTools: [] }, new AbortController().signal, new TokenMeter(100_000, () => undefined), deps({ providerKind: "sidecar", sidecar: store, systemPrompt: snapshotPrompt }));
    const storePrompt = (store.openUnattended.mock.calls[0] as unknown as [string, string])[0];
    expect(storePrompt).not.toContain("Live app context");
    expect(storePrompt).not.toContain(marked.walletAddr);
    expect(storePrompt).toContain("SCHEDULED DAEMON RUN");
    const mine = sidecarFake(done());
    await runDaemonTurn(claim, new AbortController().signal, new TokenMeter(100_000, () => undefined), deps({ providerKind: "sidecar", sidecar: mine, systemPrompt: snapshotPrompt }));
    expect((mine.openUnattended.mock.calls[0] as unknown as [string, string])[0]).toContain("Live app context");
  });
});

describe("a daemon session is opened for its daemon", () => {
  it("passes the daemon id so core re-checks a Store daemon right before opening", async () => {
    const done = () => [
      { events: [{ seq: 1, event: { type: "final", content: "ok" } }, { seq: 2, event: { type: "done", outcome: "answered" } }], lastSeq: 2, busy: false },
    ];
    const sidecar = sidecarFake(done());
    await runDaemonTurn({ ...claim, allowedTools: [] }, new AbortController().signal, new TokenMeter(100_000, () => undefined), deps({ providerKind: "sidecar", sidecar }));
    expect((sidecar.openUnattended.mock.calls[0] as unknown as [string, string, string])[2]).toBe("d1");
  });
});

describe("an open-time refusal", () => {
  it("rejects the turn as DaemonNotStarted, so the run is recorded not_started", async () => {
    const { DaemonNotStarted } = await import("./turn");
    const sidecar = sidecarFake([]);
    sidecar.openUnattended.mockRejectedValueOnce(new Error("Not started: the tools Hermes offers changed"));
    const run = runDaemonTurn({ ...claim, allowedTools: [] }, new AbortController().signal, new TokenMeter(100_000, () => undefined), deps({ providerKind: "sidecar", sidecar }));
    await expect(run).rejects.toBeInstanceOf(DaemonNotStarted);
    await expect(run).rejects.toThrow(/Not started/);
  });
});
