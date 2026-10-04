// HUP-S3.4 — "Teach Hermes": the member launches a verified workflow from the app, and only a run
// whose checks all passed lets Hermes propose what to keep. The member writes the task and the
// checks; the sidecar's verifiers judge the answer, never the model's own claim.
import { describe, it, expect } from "vitest";
import { buildTeachWorkflow, runTeach, TEACH_LIMITS, PROPOSE_PROMPT, type TeachDeps, type TeachProgress } from "./learnLauncher";
import type { WorkflowRunView, WorkflowSpec } from "./learn";
import type { SessionEventsPage } from "../bridge/domains";

describe("Feature: building the workflow the member asked for", () => {
  it("Given a task and checks, then each check is an answer_contains verifier on one step", () => {
    const r = buildTeachWorkflow({ task: "  What chain id does Citrate use?  ", checks: ["40204", "  ", "chain id"] }, 1700000000000);
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.spec.id).toBe("teach-1700000000000");
    expect(r.spec.steps).toHaveLength(1);
    const step = r.spec.steps[0];
    expect(step.instruction.startsWith("What chain id does Citrate use?")).toBe(true);
    expect(step.max_attempts).toBe(2);
    expect(step.verifiers).toEqual([
      { kind: "answer_contains", text: "40204" },
      { kind: "answer_contains", text: "chain id" },
    ]);
  });

  it("Given no task, no checks, too many checks, duplicates or overlong text, then it is refused with a reason", () => {
    const bad = [
      { task: "", checks: ["x"] },
      { task: "do it", checks: [] },
      { task: "do it", checks: ["", "  "] },
      { task: "do it", checks: Array.from({ length: TEACH_LIMITS.maxChecks + 1 }, (_, i) => `c${i}`) },
      { task: "do it", checks: ["Same", "same "] },
      { task: "x".repeat(TEACH_LIMITS.maxTask + 1), checks: ["x"] },
      { task: "do it", checks: ["y".repeat(TEACH_LIMITS.maxCheck + 1)] },
      { task: "do it\u0007", checks: ["x"] },
      { task: "do it", checks: ["x\u0000"] },
    ];
    for (const b of bad) {
      const r = buildTeachWorkflow(b);
      expect(r.ok, JSON.stringify(b).slice(0, 80)).toBe(false);
      if (!r.ok) expect(r.reason.length).toBeGreaterThan(0);
    }
  });

  it("Given an attempt count, then it is bounded to what the sidecar allows", () => {
    const one = buildTeachWorkflow({ task: "t", checks: ["c"], attempts: 1 });
    const three = buildTeachWorkflow({ task: "t", checks: ["c"], attempts: 3 });
    expect(one.ok && one.spec.steps[0].max_attempts).toBe(1);
    expect(three.ok && three.spec.steps[0].max_attempts).toBe(3);
    expect(buildTeachWorkflow({ task: "t", checks: ["c"], attempts: 9 }).ok).toBe(false);
  });
});

// ---- the run ---------------------------------------------------------------------------------

interface Fake extends TeachDeps {
  calls: string[];
  sent: string[];
}

function fake(opts: { statuses: WorkflowRunView[]; pages?: SessionEventsPage[]; openFails?: boolean; sendFails?: boolean }): Fake {
  const statuses = [...opts.statuses];
  const pages = [...(opts.pages ?? [])];
  const f: Fake = {
    calls: [],
    sent: [],
    async sessionOpen(prompt, tools) {
      f.calls.push(`open:${tools}`);
      if (opts.openFails) throw new Error("the local model isn't running yet");
      expect(prompt).toContain("Hermes");
      return "s1-ab";
    },
    async workflowRun(sid, spec: WorkflowSpec) {
      f.calls.push(`run:${sid}:${spec.id}`);
      return "wr-1";
    },
    async workflowStatus(sid, run) {
      f.calls.push(`status:${sid}:${run}`);
      return statuses.length > 1 ? (statuses.shift() as WorkflowRunView) : statuses[0];
    },
    async sessionSend(sid, text) {
      f.calls.push(`send:${sid}`);
      if (opts.sendFails) throw new Error("busy");
      f.sent.push(text);
    },
    async sessionEvents(sid, after) {
      f.calls.push(`events:${sid}:${after}`);
      return pages.shift() ?? { events: [], lastSeq: after, busy: false };
    },
    async sessionClose(sid) {
      f.calls.push(`close:${sid}`);
    },
    async sleep() {},
  };
  return f;
}

const spec = (): WorkflowSpec => {
  const r = buildTeachWorkflow({ task: "t", checks: ["40204"] }, 1);
  if (!r.ok) throw new Error(r.reason);
  return r.spec;
};

const verified: WorkflowRunView = {
  run_id: "wr-1",
  workflow_id: "teach-1",
  state: "verified",
  evidence: {
    workflow_id: "teach-1",
    steps: ["task"],
    verdicts: [{ step: "task", name: "answer mentions 40204", passed: true, detail: "" }],
    attempts: 1,
    trajectory: { session_id: "s1-ab", workflow_id: "teach-1", messages: 3, sha256: "cd".repeat(32) },
  },
  answers: ["Citrate uses chain id 40204."],
};

describe("Feature: running it", () => {
  it("Given a run whose checks all pass, then Hermes is asked to propose and the proposals are counted", async () => {
    const f = fake({
      statuses: [{ run_id: "wr-1", workflow_id: "teach-1", state: "running" }, verified],
      pages: [
        {
          events: [
            { seq: 1, event: { type: "tool_call", step: 0, call: { id: "c1", name: "learn_propose", arguments: "{}" }, host: "sidecar" } },
            { seq: 2, event: { type: "tool_result", step: 0, call_id: "c1", status: "ok", content: "{}" } },
            { seq: 3, event: { type: "tool_call", step: 1, call: { id: "c2", name: "learn_propose", arguments: "{}" }, host: "sidecar" } },
            { seq: 4, event: { type: "tool_result", step: 1, call_id: "c2", status: "error", content: "already known" } },
          ],
          lastSeq: 4,
          busy: true,
        },
        { events: [{ seq: 5, event: { type: "done", outcome: "completed" } }], lastSeq: 5, busy: false },
      ],
    });
    const seen: TeachProgress[] = [];
    const out = await runTeach(f, spec(), (p) => seen.push(p));
    expect(out.phase).toBe("done");
    expect(out.proposals).toBe(1);
    expect(out.verdicts).toEqual([{ label: "task: answer mentions 40204", passed: true }]);
    expect(f.sent).toEqual([PROPOSE_PROMPT]);
    expect(f.calls[0]).toBe("open:[]");
    expect(f.calls.at(-1)).toBe("close:s1-ab");
    expect(f.calls).toContain("events:s1-ab:4");
    expect(seen.map((p) => p.phase)).toEqual(["opening", "running", "verified", "proposing", "done"]);
  });

  it("Given a run that did not pass its checks, then nothing is proposed and the reason is shown", async () => {
    const f = fake({ statuses: [{ run_id: "wr-1", workflow_id: "teach-1", state: "unverified", reason: "step task: verifier answer mentions 40204 did not pass" }] });
    const out = await runTeach(f, spec(), () => undefined);
    expect(out.phase).toBe("unverified");
    expect(out.reason).toContain("did not pass");
    expect(f.sent).toEqual([]);
    expect(f.calls.some((c) => c.startsWith("send:"))).toBe(false);
    expect(f.calls.at(-1)).toBe("close:s1-ab");
  });

  it("Given the local model is not running, then it fails honestly and opens nothing else", async () => {
    const f = fake({ statuses: [verified], openFails: true });
    const out = await runTeach(f, spec(), () => undefined);
    expect(out.phase).toBe("failed");
    expect(out.reason).toContain("local model");
    expect(f.calls).toEqual(["open:[]"]);
  });

  it("Given a run that never finishes, then it stops waiting, says so, and still closes the session", async () => {
    const f = fake({ statuses: [{ run_id: "wr-1", workflow_id: "teach-1", state: "running" }] });
    const out = await runTeach(f, spec(), () => undefined, { maxPolls: 3 });
    expect(out.phase).toBe("failed");
    expect(out.reason).toMatch(/did not finish/);
    expect(f.calls.filter((c) => c.startsWith("status:"))).toHaveLength(3);
    expect(f.calls.at(-1)).toBe("close:s1-ab");
  });

  it("Given the propose turn never ends, then the result says how many were proposed so far", async () => {
    const f = fake({
      statuses: [verified],
      pages: [{ events: [], lastSeq: 0, busy: true }],
    });
    const out = await runTeach(f, spec(), () => undefined, { maxEventPages: 2 });
    expect(out.phase).toBe("failed");
    expect(out.reason).toMatch(/did not finish/);
    expect(out.proposals).toBe(0);
    expect(f.calls.at(-1)).toBe("close:s1-ab");
  });
});
