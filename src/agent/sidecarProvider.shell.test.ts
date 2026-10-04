// HUP-S2.2 (US-2.2) — a `shell_run` call the sidecar holds reaches the member as an approval card
// (AC2: the exact command and folder), and the decision goes back bound to exactly what was shown.
// Every command run (shell_run and the four toolchain tools) is reported as a `command_run`
// activity as well as the tool result (AC3).
import { describe, it, expect, vi } from "vitest";
import { commandRunOf, createSidecarProvider, type SidecarSessionApi } from "./sidecarProvider";
import type { TurnActivityEvent } from "./harness";
import type { ShellPendingView } from "../bridge/domains";

type Ev = { seq: number; event: Record<string, unknown> };

const PENDING: ShellPendingView = {
  id: "sh-3",
  callId: "c1",
  tool: "shell_run",
  hic: "required",
  argv: ["forge", "build", "--sizes"],
  resolvedProgram: "/opt/homebrew/bin/forge",
  cwd: "/Users/m/proj",
  timeoutSecs: 120,
  expiresInSecs: 300,
  sandbox: {
    backend: "seatbelt",
    enforced: true,
    network: "denied",
    writable: ["/Users/m/proj", "scratch HOME (deleted after the run)"],
    readable_extra: [],
    summary: "macOS Seatbelt: no network; writes only in /Users/m/proj and a scratch HOME",
  },
};

const envelope = (status: string, run: Record<string, unknown> | null, summary = "forge exited with code 0") =>
  JSON.stringify({ schema: "citrate.toolchain.v1", tool: "shell_run", status, summary, ...(run ? { run } : {}) });

const RUN = {
  program: "forge",
  args: ["build", "--sizes"],
  argv: ["forge", "build", "--sizes"],
  cwd: "/Users/m/proj",
  exit_code: 0,
  signal: null,
  timed_out: false,
  timeout_ms: 120000,
  duration_ms: 812,
  stdout: "ok",
  stderr: "",
  sandbox: PENDING.sandbox,
};

/**
 * A session api whose event pages are served in order (each page once), with shell routes. Like the
 * real sidecar, a held command produces no further events until it is decided: with `gated`, pages
 * after the first are served only once a decision was sent.
 */
function api(pages: Ev[][], pendingAfter = 0, gated = true) {
  let page = 0;
  let pendingCalls = 0;
  const a: SidecarSessionApi & { decided: unknown[] } = {
    decided: [],
    open: vi.fn(async () => "s9-beef"),
    send: vi.fn(async () => {}),
    events: vi.fn(async (_id: string, after: number) => {
      const p = pages[page];
      if (gated && page > 0 && a.decided.length === 0) return { events: [], lastSeq: after, busy: true };
      if (!p) return { events: [], lastSeq: after, busy: false };
      // A peek (after already at the page's end) does not consume the page.
      const fresh = p.filter((e) => e.seq > after);
      if (fresh.length === 0) return { events: [], lastSeq: after, busy: true };
      page += 1;
      return { events: p, lastSeq: p[p.length - 1].seq, busy: true };
    }),
    toolResult: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
    shellPending: vi.fn(async () => {
      pendingCalls += 1;
      return pendingCalls > pendingAfter ? [PENDING] : [];
    }),
    shellDecide: vi.fn(async (_id: string, approvalId: string, allow: boolean, argv: string[], cwd: string) => {
      a.decided.push({ approvalId, allow, argv, cwd });
    }),
  };
  return a;
}

const call = (seq: number, id: string, name: string, args: unknown): Ev => ({
  seq,
  event: { type: "tool_call", step: 1, call: { id, name, arguments: JSON.stringify(args) }, host: "sidecar" },
});
const result = (seq: number, id: string, status: string, content: string): Ev => ({
  seq,
  event: { type: "tool_result", step: 1, call_id: id, status, content },
});
const done = (seq: number): Ev => ({ seq, event: { type: "done", outcome: "answered" } });

const OPTS = { shellPollMs: 1, shellWaitMs: 200 };

describe("HUP-S2.2 shell_run approval in the sidecar provider", () => {
  it("polls for the held command, asks the member, and decides with exactly the argv and folder shown", async () => {
    const a = api([[call(1, "c1", "shell_run", { argv: ["forge", "build", "--sizes"], cwd: "/Users/m/proj" })], [result(2, "c1", "ok", envelope("completed", RUN)), done(3)]], 2);
    const seen: ShellPendingView[] = [];
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({
      messages: [{ role: "user", content: "build it" }],
      callbacks: {
        onStatus: vi.fn(),
        onToken: vi.fn(),
        onToolCall: vi.fn(async () => "ok"),
        onCommandApproval: async (pending) => {
          seen.push(pending);
          return true;
        },
      },
    });
    expect(seen).toEqual([PENDING]);
    expect(a.shellPending).toHaveBeenCalledTimes(3);
    expect(a.decided).toEqual([{ approvalId: "sh-3", allow: true, argv: ["forge", "build", "--sizes"], cwd: "/Users/m/proj" }]);
    // The sidecar runs it; core never posts a result for a sidecar-hosted call.
    expect(a.toolResult).not.toHaveBeenCalled();
  });

  it("a member's no is sent as a decline", async () => {
    const a = api([[call(1, "c1", "shell_run", {})], [result(2, "c1", "denied", "declined: the member declined this command. Nothing was done."), done(3)]]);
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({
      messages: [{ role: "user", content: "x" }],
      callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "ok"), onCommandApproval: async () => false },
    });
    expect(a.decided).toEqual([{ approvalId: "sh-3", allow: false, argv: PENDING.argv, cwd: PENDING.cwd }]);
  });

  it("without an approval callback the command is declined, never run silently", async () => {
    const a = api([[call(1, "c1", "shell_run", {})], [result(2, "c1", "denied", "declined: no. Nothing was done."), done(3)]]);
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({ messages: [{ role: "user", content: "x" }], callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "ok") } });
    expect(a.decided).toEqual([{ approvalId: "sh-3", allow: false, argv: PENDING.argv, cwd: PENDING.cwd }]);
  });

  it("stops waiting when the sidecar already answered the call (refused before it was held)", async () => {
    const a = api([[call(1, "c1", "shell_run", {}), result(2, "c1", "error", "tool error: " + envelope("refused", null, "the folder is not granted")), done(3)]], 1_000_000);
    const ask = vi.fn(async () => true);
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({ messages: [{ role: "user", content: "x" }], callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "ok"), onCommandApproval: ask } });
    expect(ask).not.toHaveBeenCalled();
    expect(a.decided).toEqual([]);
  });

  it("a failed decision is reported as a notice and never retried as allow", async () => {
    const a = api([[call(1, "c1", "shell_run", {})], [result(2, "c1", "denied", "declined: expired. Nothing was done."), done(3)]], 0, false);
    a.shellDecide = vi.fn(async () => {
      throw new Error("SHELL_DECISION_REFUSED: the command differs from the one waiting");
    });
    const acts: TurnActivityEvent[] = [];
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({
      messages: [{ role: "user", content: "x" }],
      callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "ok"), onCommandApproval: async () => true, onActivity: (e) => acts.push(e) },
    });
    expect(a.shellDecide).toHaveBeenCalledTimes(1);
    expect(acts.filter((e) => e.kind === "notice")).toEqual([{ kind: "notice", text: expect.stringContaining("differs") }]);
  });

  it("an api without the shell routes leaves the decision to the sidecar's own timeout", async () => {
    const a = api([[call(1, "c1", "shell_run", {})], [result(2, "c1", "denied", "declined: no decision. Nothing was done."), done(3)]], 0, false);
    delete a.shellPending;
    delete a.shellDecide;
    const ask = vi.fn(async () => true);
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({ messages: [{ role: "user", content: "x" }], callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "ok"), onCommandApproval: ask } });
    expect(ask).not.toHaveBeenCalled();
  });
});

describe("HUP-S2.2 AC3: command runs reach the Activity log", () => {
  it("reports shell_run and toolchain results as command_run activity", async () => {
    const forge = JSON.stringify({
      schema: "citrate.toolchain.v1",
      tool: "forge_test",
      status: "completed",
      summary: "12 passed, 0 failed",
      verdict: { passed: true },
      run: { program: "forge", exit_code: 0, timed_out: false, duration_ms: 4100, sandbox: PENDING.sandbox },
    });
    const a = api([
      [
        call(1, "c1", "shell_run", {}),
      ],
      [
        result(2, "c1", "ok", envelope("completed", RUN)),
        call(3, "t1", "forge_test", { project: "/Users/m/proj" }),
        result(4, "t1", "ok", forge),
        call(5, "t2", "slither_scan", { project: "/Users/m/proj" }),
        result(6, "t2", "error", "tool error: " + JSON.stringify({ schema: "x", tool: "slither_scan", status: "timed_out", summary: "slither did not finish within 300s", run: { exit_code: null, timed_out: true, duration_ms: 300000 } })),
        call(7, "c2", "shell_run", {}),
        result(8, "c2", "denied", "declined: the member declined this command. Nothing was done."),
        call(9, "x1", "skill_load", {}),
        result(10, "x1", "ok", "text"),
        done(11),
      ],
    ]);
    const acts: TurnActivityEvent[] = [];
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({
      messages: [{ role: "user", content: "x" }],
      callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "ok"), onCommandApproval: async () => true, onActivity: (e) => acts.push(e) },
    });
    const runs = acts.filter((e) => e.kind === "command_run");
    expect(runs).toEqual([
      { kind: "command_run", callId: "c1", tool: "shell_run", status: "completed", summary: "forge exited with code 0", exitCode: 0, durationMs: 812, timedOut: false, sandbox: PENDING.sandbox.summary },
      { kind: "command_run", callId: "t1", tool: "forge_test", status: "completed", summary: "12 passed, 0 failed", exitCode: 0, durationMs: 4100, timedOut: false, sandbox: PENDING.sandbox.summary },
      { kind: "command_run", callId: "t2", tool: "slither_scan", status: "timed_out", summary: "slither did not finish within 300s", exitCode: null, durationMs: 300000, timedOut: true, sandbox: null },
      { kind: "command_run", callId: "c2", tool: "shell_run", status: "declined", summary: "the member declined this command", exitCode: null, durationMs: null, timedOut: false, sandbox: null },
    ]);
  });

  it("reports a run even while a stopped turn drains", async () => {
    const ac = new AbortController();
    const a = api([[call(1, "c1", "shell_run", {})], [result(2, "c1", "ok", envelope("completed", RUN))], [{ seq: 3, event: { type: "done", outcome: "stopped" } }]]);
    const acts: TurnActivityEvent[] = [];
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    const sent = p.send({
      messages: [{ role: "user", content: "x" }],
      signal: ac.signal,
      callbacks: {
        onStatus: vi.fn(),
        onToken: vi.fn(),
        onToolCall: vi.fn(async () => "ok"),
        onCommandApproval: async () => {
          ac.abort();
          return false;
        },
        onActivity: (e) => acts.push(e),
      },
    });
    await expect(sent).rejects.toThrow();
    await vi.waitFor(() => expect(acts.some((e) => e.kind === "command_run")).toBe(true));
  });

  it("an unreadable result is still reported, honestly", () => {
    expect(commandRunOf("c9", "shell_run", "error", "tool error: something broke")).toEqual({
      kind: "command_run",
      callId: "c9",
      tool: "shell_run",
      status: "failed",
      summary: "something broke",
      exitCode: null,
      durationMs: null,
      timedOut: false,
      sandbox: null,
    });
  });
});
