// HGW-W3: a project_create result is reported to the app: a created folder (so later
// conversations send it as a project root), or a refusal because ~/Citrate/Projects is missing or
// not granted (so the app can offer the workspace card). Nothing else is read into it.
import { describe, it, expect, vi } from "vitest";
import { createSidecarProvider, projectOutcomeOf, type SidecarSessionApi } from "./sidecarProvider";
import { projectsWorkspaceCard } from "./approvalCards";
import type { TurnActivityEvent } from "./harness";

type Ev = { seq: number; event: Record<string, unknown> };

const CREATED = JSON.stringify({ ok: true, name: "demo", path: "/Users/m/Citrate/Projects/demo", template: null, marker: ".citrate/project.json", rendered: false });
const NOT_GRANTED =
  "the projects folder /Users/m/Citrate/Projects is not granted for reading and writing (no write grant); the member can allow it in the app";
const MISSING = "the projects folder /Users/m/Citrate/Projects does not exist yet; the app creates it when the member lets Hermes work there";

function api(events: Ev[]): SidecarSessionApi {
  return {
    open: vi.fn(async () => "s9-beef"),
    send: vi.fn(async () => {}),
    events: vi.fn(async (_id: string, after: number) => {
      const fresh = events.filter((e) => e.seq > after);
      return { events: fresh, lastSeq: events.length ? events[events.length - 1].seq : after, busy: true };
    }),
    toolResult: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
  };
}

const call = (seq: number, id: string, name: string): Ev => ({
  seq,
  event: { type: "tool_call", step: 1, call: { id, name, arguments: '{"name":"demo"}' }, host: "sidecar" },
});
const result = (seq: number, id: string, status: string, content: string): Ev => ({
  seq,
  event: { type: "tool_result", step: 1, call_id: id, status, content },
});
const done = (seq: number): Ev => ({ seq, event: { type: "done", outcome: "answered" } });

async function activity(events: Ev[]): Promise<TurnActivityEvent[]> {
  const seen: TurnActivityEvent[] = [];
  const p = createSidecarProvider(api(events), () => "p", () => [], () => null);
  await p.send({
    messages: [{ role: "user", content: "make a project" }],
    callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "ok"), onActivity: (ev) => seen.push(ev) },
  });
  return seen.filter((e) => e.kind === "project_created" || e.kind === "projects_workspace_needed");
}

describe("HGW-W3 project_create results in the sidecar provider", () => {
  it("a created project is reported with its absolute path", async () => {
    expect(await activity([call(1, "c1", "project_create"), result(2, "c1", "ok", CREATED), done(3)])).toEqual([
      { kind: "project_created", path: "/Users/m/Citrate/Projects/demo" },
    ]);
  });

  it("a refusal for a missing or ungranted projects folder asks for the workspace", async () => {
    expect(await activity([call(1, "c1", "project_create"), result(2, "c1", "error", NOT_GRANTED), done(3)])).toEqual([
      { kind: "projects_workspace_needed" },
    ]);
    expect(projectOutcomeOf("error", MISSING)).toEqual({ kind: "projects_workspace_needed" });
  });

  it("a decline, a bad name or another tool's result reports nothing", async () => {
    expect(projectOutcomeOf("denied", "the member declined; nothing was created")).toBeNull();
    expect(projectOutcomeOf("error", 'the project name "../x" may only use letters')).toBeNull();
    expect(projectOutcomeOf("ok", JSON.stringify({ ok: true, path: "relative/demo" }))).toBeNull();
    expect(projectOutcomeOf("ok", JSON.stringify({ ok: false, path: "/x" }))).toBeNull();
    expect(projectOutcomeOf("ok", "not json")).toBeNull();
    // The same text from any other tool is not a project result.
    expect(await activity([call(1, "c1", "fs_write"), result(2, "c1", "ok", CREATED), done(3)])).toEqual([]);
  });
});

describe("HGW-W3 workspace card", () => {
  it("names the folder, the access, the per-project card and where to revoke", () => {
    const c = projectsWorkspaceCard("/Users/m/Citrate/Projects", false);
    if (c.kind !== "fields") throw new Error("fields card expected");
    expect(c.summary).toContain("/Users/m/Citrate/Projects");
    expect(c.rows[0].v).toContain("created on Approve");
    expect(c.rows.find((r) => r.k === "Access")?.v).toContain("read and write");
    expect(c.rows.find((r) => r.k === "Revoke")?.v).toBe("Settings > Folder access");
    const existing = projectsWorkspaceCard("/Users/m/Citrate/Projects", true);
    if (existing.kind !== "fields") throw new Error("fields card expected");
    expect(existing.rows[0].v).toBe("/Users/m/Citrate/Projects");
  });
});
