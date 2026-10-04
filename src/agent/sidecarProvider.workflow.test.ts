// HUP-S3.3 (US-3.3 AC2) — track workflows run from chat through the sidecar provider.
//
// The provider starts a track's catalog workflow in its session, answers every core-hosted tool
// call through the same gated handler a chat turn uses, reports each verifier verdict, and
// finishes only when the sidecar says the run left "running" (a workflow has one `done` event per
// step attempt, so `done` alone never ends it). The persona the member chose travels with the
// session it opens.
import { describe, it, expect, vi } from "vitest";
import { createSidecarProvider, type SidecarSessionApi } from "./sidecarProvider";
import { TurnStopped, type ToolCall, type TurnActivityEvent } from "./harness";
import type { WorkflowRunView } from "./learn";

type Ev = { seq: number; event: Record<string, unknown> };
type Page = { events: Ev[]; lastSeq: number; busy: boolean };

/** Events of a status-note run: step 1 reads the journal (core), fails a verifier, retries, passes. */
const RUN: Ev[] = [
  { seq: 1, event: { type: "step_start", step: 1 } },
  { seq: 2, event: { type: "tool_call", step: 1, host: "core", call: { id: "c1", name: "journal_read", arguments: "{}" } } },
  { seq: 3, event: { type: "tool_result", step: 1, call_id: "c1", status: "ok", content: "notes" } },
  { seq: 4, event: { type: "final", content: "Done: x" } },
  { seq: 5, event: { type: "verifier", step: "note", name: "answer contains 'Next:'", passed: false, detail: "missing" } },
  { seq: 6, event: { type: "done", outcome: "answered" } },
  { seq: 7, event: { type: "final", content: "Done: x\nNext: y\nBlocked: none" } },
  { seq: 8, event: { type: "verifier", step: "note", name: "answer contains 'Next:'", passed: true, detail: "" } },
  { seq: 9, event: { type: "done", outcome: "answered" } },
];

function api(pages: Page[], finalView: WorkflowRunView) {
  const q = pages.slice();
  const posted: [string, string, string][] = [];
  let statusCalls = 0;
  const a: Required<SidecarSessionApi> = {
    open: vi.fn(async () => "sess-wf"),
    send: vi.fn(async () => {}),
    events: vi.fn(async (_id: string, after: number) => q.shift() ?? { events: [], lastSeq: after, busy: false }),
    toolResult: vi.fn(async (_id: string, callId: string, status: string, content: string) => {
      posted.push([callId, status, content]);
    }),
    stop: vi.fn(async () => {}),
    trackWorkflowRun: vi.fn(async () => ({ run_id: "wr-7" })),
    workflowStatus: vi.fn(async (): Promise<WorkflowRunView> => {
      statusCalls += 1;
      return q.length > 0 ? { run_id: "wr-7", workflow_id: finalView.workflow_id, state: "running" } : finalView;
    }),
  };
  return { a, posted, statusCalls: () => statusCalls };
}

function callbacks() {
  const ran: ToolCall[] = [];
  const activity: TurnActivityEvent[] = [];
  return {
    ran,
    activity,
    cb: {
      onStatus: vi.fn(),
      onToken: vi.fn(),
      onToolCall: vi.fn(async (c: ToolCall) => {
        ran.push(c);
        return "2026-09-30: shipped the brief screen";
      }),
      onActivity: (ev: TurnActivityEvent) => activity.push(ev),
    },
  };
}

const VERIFIED: WorkflowRunView = { run_id: "wr-7", workflow_id: "status-note", state: "verified", answers: ["Done: x\nNext: y\nBlocked: none"] };

describe("HUP-S3.3 track workflows through the sidecar provider", () => {
  it("starts the catalog workflow, answers core calls through the gated handler and returns the verdict", async () => {
    const pages: Page[] = [
      { events: RUN.slice(0, 3), lastSeq: 3, busy: true },
      { events: RUN.slice(3, 6), lastSeq: 6, busy: true },
      { events: RUN.slice(6), lastSeq: 9, busy: false },
    ];
    const { a, posted } = api(pages, VERIFIED);
    const { ran, activity, cb } = callbacks();
    const p = createSidecarProvider(a, () => "base", () => []);
    expect(p.runWorkflow).toBeTypeOf("function");
    const view = await p.runWorkflow!("status-note", { callbacks: cb });
    expect(a.trackWorkflowRun).toHaveBeenCalledWith("sess-wf", "status-note");
    expect(ran.map((c) => c.name)).toEqual(["journal_read"]);
    expect(posted).toEqual([["c1", "ok", "2026-09-30: shipped the brief screen"]]);
    expect(view.state).toBe("verified");
    // Every verdict is reported, the failed attempt included.
    const verdicts = activity.filter((e) => e.kind === "verifier");
    expect(verdicts).toHaveLength(2);
    expect(verdicts.map((v) => (v.kind === "verifier" ? v.passed : null))).toEqual([false, true]);
    expect(cb.onStatus).toHaveBeenLastCalledWith("done");
    // A step's answer is not a chat reply: the verdict and answers come from the run view.
    expect(cb.onToken).not.toHaveBeenCalled();
  });

  it("an idle session whose run still reads running is polled again, never taken as the end", async () => {
    const pages: Page[] = [
      { events: [], lastSeq: 0, busy: false },
      { events: RUN, lastSeq: 9, busy: false },
    ];
    const { a } = api(pages, VERIFIED);
    const { cb } = callbacks();
    const p = createSidecarProvider(a, () => "base", () => []);
    const view = await p.runWorkflow!("status-note", { callbacks: cb });
    expect(view.state).toBe("verified");
    expect(a.workflowStatus).toHaveBeenCalledTimes(2);
  });

  it("a step's done event does not end the run; the sidecar's run state does", async () => {
    const pages: Page[] = [
      { events: RUN.slice(0, 6), lastSeq: 6, busy: true },
      { events: [], lastSeq: 6, busy: true },
      { events: RUN.slice(6), lastSeq: 9, busy: false },
    ];
    const { a } = api(pages, VERIFIED);
    const { cb } = callbacks();
    const p = createSidecarProvider(a, () => "base", () => []);
    const view = await p.runWorkflow!("status-note", { callbacks: cb });
    expect(view.state).toBe("verified");
    expect(a.events).toHaveBeenCalledTimes(3);
  });

  it("an unverified run comes back as unverified with the sidecar's reason (never as success)", async () => {
    const bad: WorkflowRunView = { run_id: "wr-7", workflow_id: "status-note", state: "unverified", reason: "step note failed" };
    const { a } = api([{ events: RUN.slice(0, 6), lastSeq: 6, busy: false }], bad);
    const { cb } = callbacks();
    const p = createSidecarProvider(a, () => "base", () => []);
    const view = await p.runWorkflow!("status-note", { callbacks: cb });
    expect(view.state).toBe("unverified");
    expect(view.reason).toContain("failed");
  });

  it("a refused start is an error with the sidecar's reason and nothing is polled", async () => {
    const { a } = api([], VERIFIED);
    a.trackWorkflowRun = vi.fn(async () => {
      throw new Error("WORKFLOW_REFUSED: this workflow needs the contract toolchain (forge_test), which is off in this app");
    });
    const { cb } = callbacks();
    const p = createSidecarProvider(a, () => "base", () => []);
    await expect(p.runWorkflow!("contract-build", { callbacks: cb })).rejects.toThrow(/toolchain/);
    expect(a.events).not.toHaveBeenCalled();
  });

  it("Stop stops the session, reports TurnStopped and the next run opens a fresh session", async () => {
    const pages: Page[] = [{ events: RUN.slice(0, 1), lastSeq: 1, busy: true }];
    const { a } = api(pages, { run_id: "wr-7", workflow_id: "status-note", state: "unverified", reason: "stopped" });
    const { cb } = callbacks();
    const ac = new AbortController();
    // The member presses Stop once the run has started.
    a.trackWorkflowRun = vi.fn(async () => {
      ac.abort();
      return { run_id: "wr-7" };
    });
    const p = createSidecarProvider(a, () => "base", () => []);
    const run = p.runWorkflow!("status-note", { callbacks: cb, signal: ac.signal });
    await expect(run).rejects.toBeInstanceOf(TurnStopped);
    expect(a.stop).toHaveBeenCalledWith("sess-wf");
    await p.runWorkflow!("status-note", { callbacks: cb }).catch(() => undefined);
    expect(a.open).toHaveBeenCalledTimes(2);
  });

  it("an api without the workflow routes refuses honestly", async () => {
    const { a } = api([], VERIFIED);
    const { trackWorkflowRun: _t, workflowStatus: _w, ...bare } = a;
    void _t;
    void _w;
    const { cb } = callbacks();
    const p = createSidecarProvider(bare, () => "base", () => []);
    await expect(p.runWorkflow!("status-note", { callbacks: cb })).rejects.toThrow(/sidecar/);
  });

  it("the chosen persona travels with the session the provider opens (turns and workflows)", async () => {
    const { a } = api([{ events: [{ seq: 1, event: { type: "final", content: "hi" } }, { seq: 2, event: { type: "done", outcome: "answered" } }], lastSeq: 2, busy: false }], VERIFIED);
    const { cb } = callbacks();
    const p = createSidecarProvider(a, () => "base", () => [], () => ({ persona: "maker" }));
    await p.send({ messages: [{ role: "user", content: "hello" }], callbacks: cb });
    expect(a.open).toHaveBeenCalledWith("base", "[]", { persona: "maker" });
  });

  it("no persona opens the session exactly as before", async () => {
    const { a } = api([{ events: [{ seq: 1, event: { type: "final", content: "hi" } }, { seq: 2, event: { type: "done", outcome: "answered" } }], lastSeq: 2, busy: false }], VERIFIED);
    const { cb } = callbacks();
    const p = createSidecarProvider(a, () => "base", () => []);
    await p.send({ messages: [{ role: "user", content: "hello" }], callbacks: cb });
    expect(a.open).toHaveBeenCalledWith("base", "[]");
  });
});
