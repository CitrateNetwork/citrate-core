// HUP-S1 A5 (core side) — the sidecar provider driven end to end against a RECORDED session.
//
// The fixture is the real agent loop's event stream (citrate-agent-runtime agent-loop at ae29ebc,
// runtime PR #20), serialized exactly as the sidecar serves it: a 6-call reply over the per-step cap,
// malformed and non-object arguments, a denied call, an unknown tool, empty arguments, a
// sidecar-hosted call, a final answer, then a turn that fails with a transport error.
// These tests pin which core handlers run, with what arguments, and which results core posts back.
import { describe, it, expect, vi } from "vitest";
import { createSidecarProvider, type SidecarSessionApi } from "./sidecarProvider";
import type { ToolCall } from "./harness";
import recorded from "./fixtures/sidecar-session-recorded.json";

type Ev = { seq: number; event: Record<string, unknown> };
type Page = { events: Ev[]; lastSeq: number; busy: boolean };

const TURNS = recorded.turns as unknown as Ev[][];

/** Split one recorded turn into long-poll pages of `size` events; the last page is not busy. */
function paged(events: Ev[], size = 3): Page[] {
  const pages: Page[] = [];
  for (let i = 0; i < events.length; i += size) {
    const chunk = events.slice(i, i + size);
    pages.push({ events: chunk, lastSeq: chunk[chunk.length - 1].seq, busy: i + size < events.length });
  }
  return pages;
}

function sessionApi(pages: Page[]) {
  const q = pages.slice();
  const posted: { callId: string; status: string; content: string }[] = [];
  const api: SidecarSessionApi = {
    open: vi.fn(async () => "sess-rec"),
    send: vi.fn(async () => {}),
    events: vi.fn(async (_id: string, after: number) => q.shift() ?? { events: [], lastSeq: after, busy: false }),
    toolResult: vi.fn(async (_id: string, callId: string, status: string, content: string) => {
      posted.push({ callId, status, content });
    }),
    stop: vi.fn(async () => {}),
  };
  return { api, posted };
}

/** Stands in for store.handleTool: records each run and answers like the real gated handlers. */
function handlers() {
  const ran: { id: string; name: string; arguments: string }[] = [];
  const onToolCall = vi.fn(async (call: ToolCall) => {
    ran.push({ id: call.id, name: call.name, arguments: call.arguments });
    if (call.name === "node_status") return '{"height":6310}';
    if (call.name === "group_invite") return "the member declined minting an invite link; no link exists.";
    if (call.name === "journal_append") return "journal entry saved";
    return "ok";
  });
  return { ran, callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall } };
}

const user = (content: string) => ({ messages: [{ role: "user" as const, content }] });

describe("HUP-S1 A5 sidecar provider against a recorded session", () => {
  it("runs exactly the calls the loop dispatched to core, and posts exactly their results", async () => {
    const { api, posted } = sessionApi(paged(TURNS[0]));
    const { ran, callbacks } = handlers();
    const p = createSidecarProvider(api, () => "p", () => []);

    const out = await p.send({ ...user("check my node, log it, invite g-1"), callbacks });

    expect(out.content).toBe("Height 6,310. The invite was not created.");
    // c2 (malformed), c5/c6 (over the cap), c7 (unknown) carry host null; c9 is sidecar-hosted;
    // c4 was dispatched but its arguments are not an object, so core refuses it.
    expect(ran).toEqual([
      { id: "c1", name: "node_status", arguments: "{}" },
      { id: "c3", name: "group_invite", arguments: '{"group":"g-1"}' },
      { id: "c8", name: "journal_append", arguments: "{}" },
    ]);
    expect(posted.map((r) => [r.callId, r.status])).toEqual([
      ["c1", "ok"],
      ["c3", "denied"],
      ["c4", "error"],
      ["c8", "ok"],
    ]);
    expect(posted[0].content).toBe('{"height":6310}');
    expect(posted[2].content).toMatch(/JSON object/);
    expect(callbacks.onStatus).toHaveBeenLastCalledWith("done");
  });

  it("the failing turn rejects with the loop's error and runs nothing", async () => {
    const { api, posted } = sessionApi([...paged(TURNS[0]), ...paged(TURNS[1])]);
    const p = createSidecarProvider(api, () => "p", () => []);
    await p.send({ ...user("first"), callbacks: handlers().callbacks });
    const second = handlers();
    await expect(p.send({ ...user("and again"), callbacks: second.callbacks })).rejects.toThrow(/could not connect/);
    expect(second.ran).toEqual([]);
    expect(posted).toHaveLength(4);
    expect(second.callbacks.onStatus).toHaveBeenLastCalledWith("error");
  });

  it.each([
    ["an array", '["height"]'],
    ["a string", '"height"'],
    ["a number", "42"],
    ["null", "null"],
    ["unparseable text", "{not json"],
  ])("a core call whose arguments are %s is not run; core posts an error", async (_label, args) => {
    const { api, posted } = sessionApi([
      { events: [{ seq: 1, event: { type: "tool_call", step: 1, host: "core", call: { id: "x1", name: "memory_search", arguments: args } } }], lastSeq: 1, busy: true },
      { events: [{ seq: 2, event: { type: "final", content: "k" } }, { seq: 3, event: { type: "done", outcome: "answered" } }], lastSeq: 3, busy: false },
    ]);
    const { ran, callbacks } = handlers();
    await createSidecarProvider(api, () => "p", () => []).send({ ...user("x"), callbacks });
    expect(ran).toEqual([]);
    expect(posted).toEqual([{ callId: "x1", status: "error", content: expect.stringMatching(/JSON object/) }]);
  });

  it.each([
    ["an object (not a JSON string)", { q: "height" }],
    ["a number value", 7],
    ["missing", undefined],
  ])("a core call whose arguments field is %s is not run; core posts an error", async (_label, args) => {
    const { api, posted } = sessionApi([
      { events: [{ seq: 1, event: { type: "tool_call", step: 1, host: "core", call: { id: "y1", name: "memory_search", arguments: args } } }], lastSeq: 1, busy: true },
      { events: [{ seq: 2, event: { type: "final", content: "k" } }, { seq: 3, event: { type: "done", outcome: "answered" } }], lastSeq: 3, busy: false },
    ]);
    const { ran, callbacks } = handlers();
    await createSidecarProvider(api, () => "p", () => []).send({ ...user("x"), callbacks });
    expect(ran).toEqual([]);
    expect(posted).toEqual([{ callId: "y1", status: "error", content: expect.stringMatching(/JSON object/) }]);
  });

  it("a call id left waiting by a failed turn does not block the same id in the next turn", async () => {
    // Turn 1: posting c's result fails, so the turn rejects before the loop's tool_result is seen.
    const call = { id: "call_0", name: "node_status", arguments: "{}" };
    const { api, posted } = sessionApi([
      { events: [{ seq: 1, event: { type: "tool_call", step: 1, host: "core", call } }], lastSeq: 1, busy: true },
      { events: [{ seq: 2, event: { type: "tool_call", step: 1, host: "core", call } }, { seq: 3, event: { type: "tool_result", step: 1, call_id: "call_0", status: "ok", content: "{}" } }, { seq: 4, event: { type: "final", content: "k" } }, { seq: 5, event: { type: "done", outcome: "answered" } }], lastSeq: 5, busy: false },
    ]);
    const post = api.toolResult as unknown as ReturnType<typeof vi.fn>;
    post.mockImplementationOnce(async () => { throw new Error("bridge unavailable"); });
    const p = createSidecarProvider(api, () => "p", () => []);
    const first = handlers();
    await expect(p.send({ ...user("one"), callbacks: first.callbacks })).rejects.toThrow(/bridge unavailable/);
    const second = handlers();
    await p.send({ ...user("two"), callbacks: second.callbacks });
    expect(second.ran.map((r) => r.id)).toEqual(["call_0"]);
    expect(posted.map((r) => r.callId)).toEqual(["call_0"]);
  });

  it("a re-delivered page does not run its calls again", async () => {
    const pages = paged(TURNS[0]);
    // The page holding c1's tool_call arrives twice (a replay of already-seen seqs).
    const { api, posted } = sessionApi([pages[0], pages[0], ...pages.slice(1)]);
    const { ran, callbacks } = handlers();
    await createSidecarProvider(api, () => "p", () => []).send({ ...user("x"), callbacks });
    expect(ran.map((r) => r.id)).toEqual(["c1", "c3", "c8"]);
    expect(posted.filter((r) => r.callId === "c1")).toHaveLength(1);
  });

  it("a second announcement of a call still awaiting its result is ignored", async () => {
    const call = { id: "d1", name: "node_status", arguments: "{}" };
    const { api, posted } = sessionApi([
      { events: [{ seq: 1, event: { type: "tool_call", step: 1, host: "core", call } }], lastSeq: 1, busy: true },
      { events: [{ seq: 2, event: { type: "tool_call", step: 1, host: "core", call } }], lastSeq: 2, busy: true },
      { events: [{ seq: 3, event: { type: "tool_result", step: 1, call_id: "d1", status: "ok", content: "{}" } }, { seq: 4, event: { type: "final", content: "k" } }, { seq: 5, event: { type: "done", outcome: "answered" } }], lastSeq: 5, busy: false },
    ]);
    const { ran, callbacks } = handlers();
    await createSidecarProvider(api, () => "p", () => []).send({ ...user("x"), callbacks });
    expect(ran.map((r) => r.id)).toEqual(["d1"]);
    expect(posted.map((r) => r.callId)).toEqual(["d1"]);
  });

  it("a call id reused after the loop recorded its result is a new call and runs", async () => {
    // The sidecar names id-less model calls call_0, call_1, … per reply, so ids repeat across steps.
    const step = (seq: number, n: number): Ev[] => [
      { seq, event: { type: "tool_call", step: n, host: "core", call: { id: "call_0", name: "node_status", arguments: "{}" } } },
      { seq: seq + 1, event: { type: "tool_result", step: n, call_id: "call_0", status: "ok", content: "{}" } },
    ];
    const { api, posted } = sessionApi([
      { events: step(1, 1).slice(0, 1), lastSeq: 1, busy: true },
      { events: [...step(1, 1).slice(1), ...step(3, 2).slice(0, 1)], lastSeq: 3, busy: true },
      { events: [...step(3, 2).slice(1), { seq: 5, event: { type: "final", content: "k" } }, { seq: 6, event: { type: "done", outcome: "answered" } }], lastSeq: 6, busy: false },
    ]);
    const { ran, callbacks } = handlers();
    await createSidecarProvider(api, () => "p", () => []).send({ ...user("x"), callbacks });
    expect(ran.map((r) => r.id)).toEqual(["call_0", "call_0"]);
    expect(posted.map((r) => r.callId)).toEqual(["call_0", "call_0"]);
  });
});
