// HGW-W3 (core #289): the runtime's `project_create` (sidecar-hosted) reports the project it
// created; the provider hands its path to core to keep (sent back as projectRoots on later
// sessions). A refusal because the projects folder is not set up asks core for the workspace card.
// Core never runs these calls and never posts a result for them.
import { describe, it, expect, vi } from "vitest";
import { createSidecarProvider, type SidecarSessionApi } from "./sidecarProvider";

type Ev = { seq: number; event: Record<string, unknown> };

function api(events: Ev[]) {
  let served = false;
  const a: SidecarSessionApi = {
    open: vi.fn(async () => "s5-proj"),
    send: vi.fn(async () => {}),
    events: vi.fn(async (_id: string, after: number) => {
      if (served) return { events: [], lastSeq: after, busy: false };
      served = true;
      return { events, lastSeq: events[events.length - 1].seq, busy: false };
    }),
    toolResult: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
  };
  return a;
}

const call = (seq: number, id: string, name: string, host: string | null): Ev => ({
  seq,
  event: { type: "tool_call", step: 1, call: { id, name, arguments: '{"name":"demo"}' }, host },
});
const result = (seq: number, id: string, status: string, content: string): Ev => ({
  seq,
  event: { type: "tool_result", step: 1, call_id: id, status, content },
});
const created = JSON.stringify({ ok: true, tool: "project_create", name: "demo", path: "/Users/m/Citrate/Projects/demo", template: null, rendered: null });
const done = (seq: number): Ev => ({ seq, event: { type: "done", outcome: "answered" } });

async function run(events: Ev[]) {
  const a = api(events);
  const onProjectCreated = vi.fn();
  const onProjectsWorkspaceNeeded = vi.fn();
  const onToolCall = vi.fn(async () => "ok");
  const p = createSidecarProvider(a, () => "p", () => [], () => null, { onProjectCreated, onProjectsWorkspaceNeeded });
  await p.send({ messages: [{ role: "user", content: "make a project called demo" }], callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall } });
  return { a, onProjectCreated, onProjectsWorkspaceNeeded, onToolCall };
}

describe("HGW-W3 sidecar provider: project_create", () => {
  it("hands the created project's path to core, once", async () => {
    const r = await run([call(1, "p1", "project_create", "sidecar"), result(2, "p1", "ok", created), done(3)]);
    expect(r.onProjectCreated).toHaveBeenCalledTimes(1);
    expect(r.onProjectCreated).toHaveBeenCalledWith({ name: "demo", path: "/Users/m/Citrate/Projects/demo" });
    expect(r.onProjectsWorkspaceNeeded).not.toHaveBeenCalled();
    expect(r.onToolCall).not.toHaveBeenCalled();
    expect(r.a.toolResult).not.toHaveBeenCalled();
  });

  it("asks for the workspace card when the projects folder is not set up", async () => {
    const r = await run([
      call(1, "p1", "project_create", "sidecar"),
      result(2, "p1", "error", "tool error: the projects folder /Users/m/Citrate/Projects does not exist yet; the app creates it when the member lets Hermes work there"),
      done(3),
    ]);
    expect(r.onProjectsWorkspaceNeeded).toHaveBeenCalledTimes(1);
    expect(r.onProjectCreated).not.toHaveBeenCalled();
  });

  it("a declined card or another refusal records nothing and asks nothing", async () => {
    const r = await run([
      call(1, "p1", "project_create", "sidecar"),
      result(2, "p1", "denied", "declined: the member declined. Nothing was done."),
      call(3, "p2", "project_create", "sidecar"),
      result(4, "p2", "error", "tool error: /Users/m/Citrate/Projects/demo already exists; pick another name"),
      done(5),
    ]);
    expect(r.onProjectCreated).not.toHaveBeenCalled();
    expect(r.onProjectsWorkspaceNeeded).not.toHaveBeenCalled();
  });

  it("ignores a core-hosted tool of the same name (core offers none)", async () => {
    const r = await run([call(1, "c1", "project_create", "core"), result(2, "c1", "ok", created), done(3)]);
    expect(r.onProjectCreated).not.toHaveBeenCalled();
  });
});
