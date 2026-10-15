// HGW-W1: a sidecar action the sidecar holds (an effectful sidecar-hosted call after the session
// read untrusted content, or a project_create) reaches the member as an approval card, and the
// decision goes back bound to exactly the arguments shown. Untainted calls are not held up.
import { describe, it, expect, vi } from "vitest";
import { createSidecarProvider, type SidecarSessionApi } from "./sidecarProvider";
import type { ActionPendingView } from "../bridge/domains";
import { actionRequestCard } from "./approvalCards";

type Ev = { seq: number; event: Record<string, unknown> };

const ARGS = '{"content":"hello","path":"/Users/m/Citrate/Projects/demo/README.md"}';

const WRITE_CARD: ActionPendingView = {
  id: "act-1",
  callId: "c1",
  tool: "fs_write",
  hic: "required",
  subject: ARGS,
  summary: "fs_write /Users/m/Citrate/Projects/demo/README.md",
  hicReason: "this session read untrusted content (from shell_run), so this action needs your explicit approval",
  taintSources: ["shell_run"],
  expiresInSecs: 300,
};

const PROJECT_ARGS = '{"name":"demo","template":"vite-react"}';
const PROJECT_CARD: ActionPendingView = {
  id: "act-2",
  callId: "c1",
  tool: "project_create",
  hic: "required",
  subject: PROJECT_ARGS,
  summary: "project_create demo",
  hicReason: "",
  taintSources: [],
  expiresInSecs: 300,
};

/** Events served like the real sidecar; with `gated`, page n+1 is released only once n cards were
 *  decided (the sidecar holds each call until the member decides). */
function api(pages: Ev[][], cards: ActionPendingView[], gated = true) {
  const a: SidecarSessionApi & { decided: unknown[] } = {
    decided: [],
    open: vi.fn(async () => "s9-beef"),
    send: vi.fn(async () => {}),
    events: vi.fn(async (_id: string, after: number) => {
      const released = gated ? pages.slice(0, 1 + a.decided.length) : pages;
      const all = released.flat();
      const fresh = all.filter((e) => e.seq > after);
      const lastSeq = all.length ? Math.max(after, all[all.length - 1].seq) : after;
      return { events: fresh, lastSeq, busy: true };
    }),
    toolResult: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
    actionPending: vi.fn(async () => cards.filter((c) => !a.decided.some((d) => (d as { approvalId: string }).approvalId === c.id))),
    actionDecide: vi.fn(async (_id: string, approvalId: string, allow: boolean, subject: string) => {
      a.decided.push({ approvalId, allow, subject });
    }),
  };
  return a;
}

const call = (seq: number, id: string, name: string, hic = false): Ev => ({
  seq,
  event: { type: "tool_call", step: 1, call: { id, name, arguments: "{}" }, host: "sidecar", ...(hic ? { hic: "required" } : {}) },
});
const result = (seq: number, id: string, status: string, content: string): Ev => ({
  seq,
  event: { type: "tool_result", step: 1, call_id: id, status, content },
});
const done = (seq: number): Ev => ({ seq, event: { type: "done", outcome: "answered" } });

const OPTS = { actionPollMs: 1, actionWaitMs: 300 };
const cb = (onActionApproval?: (p: ActionPendingView) => Promise<boolean>) => ({
  onStatus: vi.fn(),
  onToken: vi.fn(),
  onToolCall: vi.fn(async () => "ok"),
  ...(onActionApproval ? { onActionApproval } : {}),
});

describe("HGW-W1 action approval cards in the sidecar provider", () => {
  it("asks the member about a tainted file write and decides with exactly the arguments shown", async () => {
    const a = api([[call(1, "c1", "fs_write", true)], [result(2, "c1", "ok", "wrote 5 bytes"), done(3)]], [WRITE_CARD]);
    const seen: ActionPendingView[] = [];
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({
      messages: [{ role: "user", content: "write the readme" }],
      callbacks: cb(async (pending) => {
        seen.push(pending);
        return true;
      }),
    });
    expect(seen).toEqual([WRITE_CARD]);
    expect(a.decided).toEqual([{ approvalId: "act-1", allow: true, subject: ARGS }]);
    expect(a.toolResult).not.toHaveBeenCalled();
  });

  it("asks about each of several actions after taint: one approval releases one call", async () => {
    const second: ActionPendingView = { ...WRITE_CARD, id: "act-3", callId: "c2" };
    const a = api(
      [[call(1, "c1", "fs_write", true)], [result(2, "c1", "ok", "wrote"), call(3, "c2", "fs_write", true)], [result(4, "c2", "ok", "wrote"), done(5)]],
      [WRITE_CARD, second],
    );
    // The second card appears only after the first was decided.
    a.actionPending = vi.fn(async () => {
      const left = [WRITE_CARD, second].filter((c) => !a.decided.some((d) => (d as { approvalId: string }).approvalId === c.id));
      return left.slice(0, 1);
    });
    const ask = vi.fn(async () => true);
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({ messages: [{ role: "user", content: "x" }], callbacks: cb(ask) });
    expect(ask).toHaveBeenCalledTimes(2);
    expect(a.decided.map((d) => (d as { approvalId: string }).approvalId)).toEqual(["act-1", "act-3"]);
  });

  it("a project_create always asks, even in an untainted session", async () => {
    const a = api([[call(1, "c1", "project_create")], [result(2, "c1", "ok", '{"ok":true}'), done(3)]], [PROJECT_CARD]);
    const ask = vi.fn(async () => true);
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({ messages: [{ role: "user", content: "make a project" }], callbacks: cb(ask) });
    expect(ask).toHaveBeenCalledWith(PROJECT_CARD);
    expect(a.decided).toEqual([{ approvalId: "act-2", allow: true, subject: PROJECT_ARGS }]);
  });

  it("a no is a decline, and without an approval callback every held action is declined", async () => {
    const a = api([[call(1, "c1", "fs_write", true)], [result(2, "c1", "denied", "declined"), done(3)]], [WRITE_CARD]);
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({ messages: [{ role: "user", content: "x" }], callbacks: cb(async () => false) });
    expect(a.decided).toEqual([{ approvalId: "act-1", allow: false, subject: ARGS }]);
    const b = api([[call(1, "c1", "fs_write", true)], [result(2, "c1", "denied", "declined"), done(3)]], [WRITE_CARD]);
    const q = createSidecarProvider(b, () => "p", () => [], () => null, OPTS);
    await q.send({ messages: [{ role: "user", content: "x" }], callbacks: cb() });
    expect(b.decided).toEqual([{ approvalId: "act-1", allow: false, subject: ARGS }]);
  });

  it("an untainted sidecar call is not held up: no card is looked for", async () => {
    const a = api([[call(1, "c1", "fs_read"), result(2, "c1", "ok", "text"), done(3)]], [], false);
    const ask = vi.fn(async () => true);
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({ messages: [{ role: "user", content: "read" }], callbacks: cb(ask) });
    expect(ask).not.toHaveBeenCalled();
    expect(a.actionPending).not.toHaveBeenCalled();
  });

  it("shell_run and MCP calls keep their own cards, never the action cards", async () => {
    const a = api([[call(1, "c1", "shell_run", true), result(2, "c1", "denied", "declined"), call(3, "c2", "mcp__node__tx_propose", true), result(4, "c2", "denied", "x"), done(5)]], [], false);
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({ messages: [{ role: "user", content: "x" }], callbacks: cb(async () => true) });
    expect(a.actionPending).not.toHaveBeenCalled();
  });

  it("a decision that does not reach Hermes is reported, not retried as an approval", async () => {
    const a = api([[call(1, "c1", "fs_write", true)], [result(2, "c1", "denied", "declined"), done(3)]], [WRITE_CARD]);
    a.actionDecide = vi.fn(async (_id: string, approvalId: string, allow: boolean, subject: string) => {
      a.decided.push({ approvalId, allow, subject, refused: true });
      throw new Error("ACTION_DECISION_REFUSED: no longer waiting");
    });
    const notices: string[] = [];
    const p = createSidecarProvider(a, () => "p", () => [], () => null, OPTS);
    await p.send({
      messages: [{ role: "user", content: "x" }],
      callbacks: { ...cb(async () => true), onActivity: (ev) => ev.kind === "notice" && notices.push(ev.text) },
    });
    expect(a.actionDecide).toHaveBeenCalledTimes(1);
    expect(notices[0]).toContain("did not reach Hermes");
  });
});

describe("HGW-W1 action approval card content", () => {
  it("lists the tool, the exact arguments and every taint source", () => {
    const c = actionRequestCard(WRITE_CARD);
    if (c.kind !== "fields") throw new Error("fields card expected");
    expect(c.rows[0]).toEqual({ k: "Tool", v: "fs_write" });
    expect(c.rows).toContainEqual({ k: "Argument path", v: "/Users/m/Citrate/Projects/demo/README.md" });
    expect(c.rows).toContainEqual({ k: "Outside content from", v: "shell_run" });
    expect(c.summary).toContain("README.md");
  });

  it("a project card names the project and has no taint row when the session is clean", () => {
    const c = actionRequestCard(PROJECT_CARD);
    if (c.kind !== "fields") throw new Error("fields card expected");
    expect(c.summary).toContain("create the project demo");
    expect(c.rows.some((r) => r.k === "Outside content from")).toBe(false);
    expect(c.rows).toContainEqual({ k: "Argument template", v: "vite-react" });
  });

  it("arguments that are not JSON are shown raw, never reinterpreted", () => {
    const c = actionRequestCard({ ...WRITE_CARD, subject: "not json" });
    if (c.kind !== "fields") throw new Error("fields card expected");
    expect(c.rows).toContainEqual({ k: "Argument arguments", v: "not json" });
  });
});
