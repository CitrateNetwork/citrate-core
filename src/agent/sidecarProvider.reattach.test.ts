// HUP-S1.1 (US-1.1 AC3, g1-loop) and g1-render — the view saves its session, a reloaded view picks
// it back up from the saved sequence number, and streamed text is shown once.
//
// The fake below behaves like the sidecar's session routes: one event log with sequence numbers,
// `events(after)` returns only events after `after`, a core call waits until its result is posted
// (409 once nobody waits), and a session the sidecar no longer has answers 404.
import { describe, it, expect, vi } from "vitest";
import {
  createSidecarProvider,
  INTERRUPTED_RESULT,
  localSessionStore,
  parseSavedSession,
  SESSION_GONE_NOTICE,
  type SavedSidecarSession,
  type SidecarSessionApi,
  type SidecarSessionStore,
} from "./sidecarProvider";
import type { ReattachResult, ToolCall } from "./harness";

type Ev = Record<string, unknown>;

/** A scripted turn: events before a core call, the call, and what follows its result. */
interface Script {
  before: Ev[];
  call?: { id: string; name: string };
  after: Ev[];
}

function fakeSidecar(id = "s1-18f0a") {
  const log: { seq: number; event: Ev }[] = [];
  let busy = false;
  const waiting = new Set<string>();
  let onResult: ((callId: string, status: string, content: string) => void) | null = null;
  let gone = false;
  const eventsCalls: number[] = [];
  const posted: { callId: string; status: string; content: string }[] = [];
  const push = (event: Ev) => log.push({ seq: log.length + 1, event });

  function run(s: Script) {
    busy = true;
    s.before.forEach(push);
    if (s.call) {
      push({ type: "tool_call", step: 1, host: "core", call: { id: s.call.id, name: s.call.name, arguments: "{}" } });
      waiting.add(s.call.id);
      onResult = (callId, status, content) => {
        push({ type: "tool_result", step: 1, call_id: callId, status, content });
        s.after.forEach(push);
        busy = false;
      };
    } else {
      s.after.forEach(push);
      busy = false;
    }
  }

  const api: SidecarSessionApi = {
    open: vi.fn(async () => id),
    send: vi.fn(async (sid: string) => {
      if (gone || sid !== id) throw new Error("hermes control returned 404: ");
      if (busy) throw new Error("hermes control returned 409: busy");
    }),
    events: vi.fn(async (sid: string, after: number) => {
      if (gone || sid !== id) throw new Error("hermes control returned 404: ");
      eventsCalls.push(after);
      return { events: log.filter((e) => e.seq > after), lastSeq: log.length, busy, pendingCoreCalls: [...waiting] };
    }),
    position: vi.fn(async (sid: string) => {
      if (gone || sid !== id) throw new Error("hermes control returned 404: ");
      return log.length;
    }),
    toolResult: vi.fn(async (_sid: string, callId: string, status: string, content: string) => {
      if (!waiting.delete(callId)) throw new Error("hermes control returned 409: ");
      posted.push({ callId, status, content });
      onResult?.(callId, status, content);
    }),
    stop: vi.fn(async () => {}),
  };
  return {
    api,
    log,
    posted,
    eventsCalls,
    run,
    restart: () => {
      gone = true;
    },
  };
}

function memoryStore(): SidecarSessionStore & { value: SavedSidecarSession | null } {
  const s = {
    value: null as SavedSidecarSession | null,
    load: () => (s.value ? { ...s.value, inFlight: [...s.value.inFlight] } : null),
    save: (v: SavedSidecarSession | null) => {
      s.value = v ? { ...v, inFlight: [...v.inFlight] } : null;
    },
  };
  return s;
}

function callbacks(handler?: (call: ToolCall) => Promise<string>) {
  let text = "";
  const notices: string[] = [];
  const ran: string[] = [];
  const cb = {
    onStatus: vi.fn(),
    onToken: vi.fn((t: string) => {
      text += t;
    }),
    onToolCall: vi.fn(async (call: ToolCall) => {
      ran.push(call.id);
      return handler ? handler(call) : '{"height":6310}';
    }),
    onActivity: vi.fn((ev: { kind: string; text?: string }) => {
      if (ev.kind === "notice" && ev.text) notices.push(ev.text);
    }),
  };
  return { cb, text: () => text, notices, ran };
}

const user = (content: string) => [{ role: "user", content }];
const flush = () => new Promise((r) => setTimeout(r, 0));

describe("HUP-S1.1 the view picks its session back up after a reload", () => {
  it("a view torn down mid-turn is replaced by one that continues from the saved seq: no repeats, no gaps", async () => {
    const sc = fakeSidecar();
    const store = memoryStore();
    // Turn: step 1 streams a little and calls node_status (core); after the result, step 2 answers.
    const script: Script = {
      before: [{ type: "step_start", step: 1 }, { type: "assistant_delta", step: 1, text: "Checking. " }],
      call: { id: "c1", name: "node_status" },
      after: [
        { type: "step_end", step: 1 },
        { type: "step_start", step: 2 },
        { type: "assistant_delta", step: 2, text: "Height " },
        { type: "assistant_delta", step: 2, text: "6,310." },
        { type: "final", content: "Height 6,310." },
        { type: "done", outcome: "answered" },
      ],
    };
    const send = sc.api.send as unknown as ReturnType<typeof vi.fn>;
    send.mockImplementationOnce(async () => {
      sc.run(script);
    });

    // View A: the member's approval for node_status never comes back, because the view goes away.
    const a = callbacks(() => new Promise<string>(() => {}));
    const viewA = createSidecarProvider(sc.api, () => "p", () => [], () => null, { store });
    void viewA.send({ messages: user("height?"), callbacks: a.cb }).catch(() => undefined);
    for (let i = 0; i < 20 && a.ran.length === 0; i++) await flush();
    expect(a.ran).toEqual(["c1"]);
    expect(store.value).toEqual({ v: 1, id: "s1-18f0a", lastSeq: 3, inFlight: ["c1"] });
    expect(a.text()).toBe("Checking. ");

    // View B: a new provider over the same saved state (the webview reloaded).
    const b = callbacks();
    const viewB = createSidecarProvider(sc.api, () => "p", () => [], () => null, { store });
    const before = sc.eventsCalls.length;
    const r = (await viewB.reattach!({ callbacks: b.cb })) as Extract<ReattachResult, { kind: "resumed" }>;

    expect(r.kind).toBe("resumed");
    expect(r.content).toBe("Height 6,310.");
    expect(r.interrupted).toBe(1);
    expect(r.failure).toBeNull();
    // Every read after the reload starts at the saved seq or later.
    expect(sc.eventsCalls.slice(before)[0]).toBe(3);
    expect(sc.eventsCalls.slice(before).every((after) => after >= 3)).toBe(true);
    // The call view A had started is closed as interrupted, never run again by anyone.
    expect(a.ran).toEqual(["c1"]);
    expect(b.ran).toEqual([]);
    expect(sc.posted).toEqual([{ callId: "c1", status: "error", content: INTERRUPTED_RESULT }]);
    expect(b.notices.join(" ")).toMatch(/interrupted, with an unknown outcome/);
    // The answer is shown exactly once, built from the stream.
    expect(b.text()).toBe("Height 6,310.");
    expect(b.cb.onStatus).toHaveBeenLastCalledWith("done");
    // The saved position is the end of the log, with nothing in flight.
    expect(store.value).toEqual({ v: 1, id: "s1-18f0a", lastSeq: sc.log.length, inFlight: [] });
  });

  it("a core call announced after the saved point runs in the reloaded view (resumed, not lost)", async () => {
    const sc = fakeSidecar();
    const store = memoryStore();
    store.save({ v: 1, id: "s1-18f0a", lastSeq: 1, inFlight: [] });
    sc.run({
      before: [{ type: "step_start", step: 1 }],
      call: { id: "c7", name: "node_status" },
      after: [{ type: "final", content: "Height 6,310." }, { type: "done", outcome: "answered" }],
    });
    const b = callbacks();
    const view = createSidecarProvider(sc.api, () => "p", () => [], () => null, { store });
    const r = await view.reattach!({ callbacks: b.cb });
    expect(r.kind).toBe("resumed");
    expect(b.ran).toEqual(["c7"]);
    expect(sc.posted).toEqual([{ callId: "c7", status: "ok", content: '{"height":6310}' }]);
    expect(b.text()).toBe("Height 6,310.");
  });

  it("a call the view had started is never run again, even when closing it as interrupted fails", async () => {
    const sc = fakeSidecar();
    const store = memoryStore();
    sc.run({
      before: [{ type: "step_start", step: 1 }],
      call: { id: "c1", name: "group_invite" },
      after: [{ type: "final", content: "The invite was not confirmed." }, { type: "done", outcome: "answered" }],
    });
    // Saved before the call's event was marked processed (an older save), with c1 started.
    store.save({ v: 1, id: "s1-18f0a", lastSeq: 1, inFlight: ["c1"] });
    const post = sc.api.toolResult as unknown as ReturnType<typeof vi.fn>;
    const real = post.getMockImplementation()!;
    post.mockImplementationOnce(async () => {
      throw new Error("hermes control transport error: connection reset");
    });
    // The loop's own deadline answers c1 after a few polls.
    const events = sc.api.events as unknown as ReturnType<typeof vi.fn>;
    const realEvents = events.getMockImplementation()!;
    let polls = 0;
    events.mockImplementation(async (sid: string, after: number) => {
      polls += 1;
      if (polls === 3) await real(sid, "c1", "error", "the app did not answer within 300s");
      return realEvents(sid, after);
    });
    const b = callbacks();
    const r = await createSidecarProvider(sc.api, () => "p", () => [], () => null, { store }).reattach!({ callbacks: b.cb });
    expect(r).toMatchObject({ kind: "resumed", content: "The invite was not confirmed.", interrupted: 0 });
    expect(b.ran).toEqual([]);
  });

  it("a turn that finished while the view was gone is shown, and its answered calls are not re-run", async () => {
    const sc = fakeSidecar();
    const store = memoryStore();
    store.save({ v: 1, id: "s1-18f0a", lastSeq: 0, inFlight: [] });
    // The loop gave up waiting on c2 (timed out) and answered anyway.
    sc.run({
      before: [
        { type: "step_start", step: 1 },
        { type: "tool_call", step: 1, host: "core", call: { id: "c2", name: "group_invite", arguments: "{}" } },
        { type: "tool_result", step: 1, call_id: "c2", status: "error", content: "the app did not answer within 300s" },
        { type: "final", content: "I could not create the invite." },
        { type: "done", outcome: "answered" },
      ],
      after: [],
    });
    const b = callbacks();
    const r = await createSidecarProvider(sc.api, () => "p", () => [], () => null, { store }).reattach!({ callbacks: b.cb });
    expect(r).toMatchObject({ kind: "resumed", content: "I could not create the invite." });
    expect(b.ran).toEqual([]);
    expect(sc.posted).toEqual([]);
  });

  it("an idle saved session is adopted: the next message continues it without opening another", async () => {
    const sc = fakeSidecar();
    const store = memoryStore();
    sc.run({ before: [{ type: "step_start", step: 1 }, { type: "final", content: "hi" }, { type: "done", outcome: "answered" }], after: [] });
    store.save({ v: 1, id: "s1-18f0a", lastSeq: 3, inFlight: [] });
    const view = createSidecarProvider(sc.api, () => "p", () => [], () => null, { store });
    expect(await view.reattach!({ callbacks: callbacks().cb })).toEqual({ kind: "idle", sessionId: "s1-18f0a" });
    const send = sc.api.send as unknown as ReturnType<typeof vi.fn>;
    send.mockImplementationOnce(async () => {
      sc.run({ before: [{ type: "step_start", step: 1 }, { type: "final", content: "again" }, { type: "done", outcome: "answered" }], after: [] });
    });
    const c = callbacks();
    const out = await view.send({ messages: user("again?"), callbacks: c.cb });
    expect(out.content).toBe("again");
    expect(sc.api.open).not.toHaveBeenCalled();
    expect(c.text()).toBe("again");
  });

  it("stale session (Hermes restarted): the saved state is cleared and the member is told plainly", async () => {
    const sc = fakeSidecar();
    const store = memoryStore();
    store.save({ v: 1, id: "s1-18f0a", lastSeq: 9, inFlight: ["c1"] });
    sc.restart();
    const view = createSidecarProvider(sc.api, () => "p", () => [], () => null, { store });
    expect(await view.reattach!({ callbacks: callbacks().cb })).toEqual({ kind: "gone", notice: SESSION_GONE_NOTICE });
    expect(store.value).toBeNull();
    expect(sc.posted).toEqual([]);
  });

  it("a saved seq beyond the session's log means another session: cleared, never attached", async () => {
    const sc = fakeSidecar();
    const store = memoryStore();
    store.save({ v: 1, id: "s1-18f0a", lastSeq: 50, inFlight: [] });
    const r = await createSidecarProvider(sc.api, () => "p", () => [], () => null, { store }).reattach!({ callbacks: callbacks().cb });
    expect(r.kind).toBe("gone");
    expect(store.value).toBeNull();
  });

  it("Hermes not answering keeps the saved state for the next try", async () => {
    const store = memoryStore();
    store.save({ v: 1, id: "s1-18f0a", lastSeq: 2, inFlight: [] });
    const api: SidecarSessionApi = {
      open: vi.fn(),
      send: vi.fn(),
      events: vi.fn(async () => {
        throw new Error("hermes is not running (no session bearer)");
      }),
      toolResult: vi.fn(),
      stop: vi.fn(),
    };
    const r = await createSidecarProvider(api, () => "p", () => [], () => null, { store }).reattach!({ callbacks: callbacks().cb });
    expect(r.kind).toBe("unavailable");
    expect(store.value).not.toBeNull();
  });

  it("nothing saved: nothing to pick up", async () => {
    const sc = fakeSidecar();
    expect(await createSidecarProvider(sc.api, () => "p", () => [], () => null, { store: memoryStore() }).reattach!({ callbacks: callbacks().cb })).toEqual({ kind: "none" });
  });

  it("a message to a session Hermes no longer has continues in a new one, with a notice", async () => {
    const sc = fakeSidecar();
    const store = memoryStore();
    const view = createSidecarProvider(sc.api, () => "p", () => [], () => null, { store });
    const send = sc.api.send as unknown as ReturnType<typeof vi.fn>;
    send.mockImplementationOnce(async () => {
      sc.run({ before: [{ type: "final", content: "one" }, { type: "done", outcome: "answered" }], after: [] });
    });
    await view.send({ messages: user("one"), callbacks: callbacks().cb });
    // Hermes restarts; the next session it opens has a new id.
    const fresh = fakeSidecar("s2-19aa0");
    sc.restart();
    (sc.api.open as unknown as ReturnType<typeof vi.fn>).mockImplementation(async () => "s2-19aa0");
    (sc.api.send as unknown as ReturnType<typeof vi.fn>).mockImplementation(async (sid: string) => {
      if (sid !== "s2-19aa0") throw new Error("hermes control returned 404: ");
      fresh.run({ before: [{ type: "final", content: "two" }, { type: "done", outcome: "answered" }], after: [] });
    });
    sc.api.events = fresh.api.events;
    sc.api.position = fresh.api.position;
    const c = callbacks();
    const out = await view.send({ messages: user("two"), callbacks: c.cb });
    expect(out.content).toBe("two");
    expect(c.notices).toContain(SESSION_GONE_NOTICE);
    expect(store.value?.id).toBe("s2-19aa0");
  });

  it("a turn another client ran in the session while this view was idle is not taken as this turn", async () => {
    const sc = fakeSidecar();
    const view = createSidecarProvider(sc.api, () => "p", () => [], () => null, { store: memoryStore() });
    const send = sc.api.send as unknown as ReturnType<typeof vi.fn>;
    send.mockImplementationOnce(async () => {
      sc.run({ before: [{ type: "final", content: "mine 1" }, { type: "done", outcome: "answered" }], after: [] });
    });
    await view.send({ messages: user("1"), callbacks: callbacks().cb });
    // The CLI (or an MCP client) runs a turn in the same session.
    sc.run({ before: [{ type: "step_start", step: 1 }, { type: "final", content: "from the CLI" }, { type: "done", outcome: "answered" }], after: [] });
    send.mockImplementationOnce(async () => {
      sc.run({ before: [{ type: "step_start", step: 1 }, { type: "final", content: "mine 2" }, { type: "done", outcome: "answered" }], after: [] });
    });
    const c = callbacks();
    const out = await view.send({ messages: user("2"), callbacks: c.cb });
    expect(out.content).toBe("mine 2");
    expect(c.text()).toBe("mine 2");
  });

  it("a stopped session is forgotten, so a reload does not attach to it", async () => {
    const sc = fakeSidecar();
    const store = memoryStore();
    const send = sc.api.send as unknown as ReturnType<typeof vi.fn>;
    send.mockImplementationOnce(async () => {
      sc.run({ before: [{ type: "done", outcome: "stopped" }], after: [] });
    });
    const view = createSidecarProvider(sc.api, () => "p", () => [], () => null, { store });
    await expect(view.send({ messages: user("x"), callbacks: callbacks().cb })).rejects.toThrow(/stopped/);
    expect(store.value).toBeNull();
  });

  it("the app's store keeps one small localStorage entry and forgets it on save(null)", () => {
    localStorage.clear();
    const st = localSessionStore("test.hermes.session");
    expect(st.load()).toBeNull();
    st.save({ v: 1, id: "s1-ab", lastSeq: 7, inFlight: ["c1"] });
    expect(JSON.parse(localStorage.getItem("test.hermes.session") ?? "null")).toEqual({ v: 1, id: "s1-ab", lastSeq: 7, inFlight: ["c1"] });
    expect(st.load()).toEqual({ v: 1, id: "s1-ab", lastSeq: 7, inFlight: ["c1"] });
    localStorage.setItem("test.hermes.session", "{broken");
    expect(st.load()).toBeNull();
    st.save(null);
    expect(localStorage.getItem("test.hermes.session")).toBeNull();
  });

  it("only the exact saved shape is read back", () => {
    expect(parseSavedSession({ v: 1, id: "s1-ab", lastSeq: 4, inFlight: ["c1"] })).toEqual({ v: 1, id: "s1-ab", lastSeq: 4, inFlight: ["c1"] });
    for (const bad of [null, "x", { v: 2, id: "s1", lastSeq: 1, inFlight: [] }, { v: 1, id: "../x", lastSeq: 1, inFlight: [] }, { v: 1, id: "s1", lastSeq: -1, inFlight: [] }, { v: 1, id: "s1", lastSeq: 1.5, inFlight: [] }, { v: 1, id: "s1", lastSeq: 1, inFlight: [3] }]) {
      expect(parseSavedSession(bad)).toBeNull();
    }
  });
});

describe("HUP-S1.1 g1-render: streamed text in the chat", () => {
  function pagedApi(pages: { events: { seq: number; event: Ev }[]; lastSeq: number; busy: boolean }[]) {
    const q = pages.slice();
    const api: SidecarSessionApi = {
      open: vi.fn(async () => "s1-ab"),
      send: vi.fn(async () => {}),
      events: vi.fn(async (_id: string, after: number) => q.shift() ?? { events: [], lastSeq: after, busy: false }),
      toolResult: vi.fn(async () => {}),
      stop: vi.fn(async () => {}),
    };
    return api;
  }

  it("deltas are shown as they arrive and the final event adds nothing twice", async () => {
    const api = pagedApi([
      { events: [{ seq: 1, event: { type: "step_start", step: 1 } }, { seq: 2, event: { type: "assistant_delta", step: 1, text: "Your node " } }], lastSeq: 2, busy: true },
      { events: [{ seq: 3, event: { type: "assistant_delta", step: 1, text: "is validating." } }, { seq: 4, event: { type: "final", content: "Your node is validating." } }, { seq: 5, event: { type: "done", outcome: "answered" } }], lastSeq: 5, busy: false },
    ]);
    const c = callbacks();
    const out = await createSidecarProvider(api, () => "p", () => []).send({ messages: user("status?"), callbacks: c.cb });
    expect(out.content).toBe("Your node is validating.");
    expect(c.cb.onToken.mock.calls.map((x) => x[0])).toEqual(["Your node ", "is validating."]);
    expect(c.text()).toBe("Your node is validating.");
  });

  it("text that led into a tool call stays, and the answer starts a new paragraph", async () => {
    const api = pagedApi([
      {
        events: [
          { seq: 1, event: { type: "step_start", step: 1 } },
          { seq: 2, event: { type: "assistant_delta", step: 1, text: "Let me check." } },
          { seq: 3, event: { type: "tool_call", step: 1, host: "core", call: { id: "c1", name: "node_status", arguments: "{}" } } },
        ],
        lastSeq: 3,
        busy: true,
      },
      {
        events: [
          { seq: 4, event: { type: "tool_result", step: 1, call_id: "c1", status: "ok", content: "{}" } },
          { seq: 5, event: { type: "step_start", step: 2 } },
          { seq: 6, event: { type: "assistant_delta", step: 2, text: "Height 6,310." } },
          { seq: 7, event: { type: "final", content: "Height 6,310." } },
          { seq: 8, event: { type: "done", outcome: "answered" } },
        ],
        lastSeq: 8,
        busy: false,
      },
    ]);
    const c = callbacks();
    const out = await createSidecarProvider(api, () => "p", () => []).send({ messages: user("height?"), callbacks: c.cb });
    expect(out.content).toBe("Height 6,310.");
    expect(c.text()).toBe("Let me check.\n\nHeight 6,310.");
  });

  it("without deltas (an older sidecar) the final answer is shown whole, as before", async () => {
    const api = pagedApi([{ events: [{ seq: 1, event: { type: "final", content: "Whole." } }, { seq: 2, event: { type: "done", outcome: "answered" } }], lastSeq: 2, busy: false }]);
    const c = callbacks();
    await createSidecarProvider(api, () => "p", () => []).send({ messages: user("x"), callbacks: c.cb });
    expect(c.cb.onToken.mock.calls.map((x) => x[0])).toEqual(["Whole."]);
  });

  it("a workflow run shows verdicts, not streamed step text", async () => {
    const api = pagedApi([
      {
        events: [
          { seq: 1, event: { type: "assistant_delta", step: 1, text: "step text" } },
          { seq: 2, event: { type: "final", content: "step text" } },
          { seq: 3, event: { type: "done", outcome: "answered" } },
        ],
        lastSeq: 3,
        busy: false,
      },
    ]);
    api.trackWorkflowRun = vi.fn(async () => ({ run_id: "r1" }));
    api.workflowStatus = vi.fn(async () => ({ run_id: "r1", workflow_id: "w", state: "verified" }) as never);
    const c = callbacks();
    await createSidecarProvider(api, () => "p", () => []).runWorkflow!("w", { callbacks: c.cb });
    expect(c.cb.onToken).not.toHaveBeenCalled();
  });
});
