// HUP-S1.1 (US-1.1 AC3) — the store's reattach after a reload, through the app's own session store
// (localStorage) and the store's resume path: the saved {id, lastSeq, inFlight} survives the
// reload, the call that was running is reported as interrupted (never re-run), and the turn is
// followed to its end from the saved sequence number.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import { turnActivity, IDLE_ACTIVITY } from "./slices/turnActivity";
import { createSidecarProvider, INTERRUPTED_RESULT, localSessionStore, parseSavedSession, type SidecarSessionApi } from "../agent/sidecarProvider";
import type { ChatProvider } from "../agent/harness";

const KEY = "citrate.hermes.sidecarSession.v1";
const flush = () => new Promise((r) => setTimeout(r, 0));
type Ev = Record<string, unknown>;

/** The sidecar's session routes for one scripted turn: a core call, then a final answer. */
function fakeSidecar(id = "s1-77aa") {
  const log: { seq: number; event: Ev }[] = [];
  let busy = false;
  const waiting = new Set<string>();
  const posted: { callId: string; status: string; content: string }[] = [];
  const push = (event: Ev) => log.push({ seq: log.length + 1, event });
  const api: SidecarSessionApi = {
    open: vi.fn(async () => id),
    send: vi.fn(async () => {
      busy = true;
      push({ type: "step", step: 1 });
      push({ type: "tool_call", step: 1, host: "core", call: { id: "call_0", name: "node_status", arguments: "{}" } });
      waiting.add("call_0");
    }),
    events: vi.fn(async (_sid: string, after: number) => ({
      events: log.filter((e) => e.seq > after),
      lastSeq: log.length,
      busy,
      pendingCoreCalls: [...waiting],
    })),
    position: vi.fn(async () => log.length),
    toolResult: vi.fn(async (_sid: string, callId: string, status: string, content: string) => {
      if (!waiting.delete(callId)) throw new Error("hermes control returned 409: ");
      posted.push({ callId, status, content });
      push({ type: "tool_result", step: 1, call_id: callId, status, content });
      push({ type: "final", content: "Your node could not be read just now, so I cannot say its height." });
      push({ type: "done", outcome: "answered" });
      busy = false;
    }),
    stop: vi.fn(async () => {}),
  };
  return { api, posted, log };
}

const provider = (api: SidecarSessionApi): ChatProvider =>
  createSidecarProvider(api, () => "You are Hermes.", () => [], () => null, { store: localSessionStore() });

beforeEach(() => {
  localStorage.removeItem(KEY);
  store.setState({ chatMsgs: [], chatStatus: "ready" });
  turnActivity.set(IDLE_ACTIVITY);
});
afterEach(() => {
  vi.restoreAllMocks();
  localStorage.removeItem(KEY);
});

describe("HUP-S1.1 store reattach after a reload", () => {
  it("saves {id, lastSeq, inFlight} while a core call runs, and picks the turn back up after the reload", async () => {
    const sc = fakeSidecar();
    // The core call is still running (an approval card, a slow read) when the view reloads.
    const handle = vi.spyOn(store, "handleTool").mockImplementation(() => new Promise<string>(() => {}));
    store.provider = provider(sc.api);
    void store.sendChat("what is my node's height?");
    for (let i = 0; i < 20 && handle.mock.calls.length === 0; i++) await flush();
    expect(handle).toHaveBeenCalledTimes(1);

    // What survives the reload: exactly the saved shape, read back the way the app reads it.
    const saved = parseSavedSession(JSON.parse(localStorage.getItem(KEY) ?? "null"));
    // lastSeq is the call's own event: saved before the call runs, so it is never run twice.
    expect(saved).toEqual({ v: 1, id: "s1-77aa", lastSeq: 2, inFlight: ["call_0"] });

    // The reload: a fresh view with an empty chat, a new provider, the same localStorage.
    store.setState({ chatMsgs: [], chatStatus: "ready" });
    turnActivity.set(IDLE_ACTIVITY);
    handle.mockClear();
    const before = (sc.api.events as ReturnType<typeof vi.fn>).mock.calls.length;
    const reloaded = provider(sc.api);
    store.provider = reloaded;
    await (store as unknown as { resumeSidecarSession(p: ChatProvider): Promise<void> }).resumeSidecarSession(reloaded);

    // The running call is reported as interrupted (its outcome is unknown), never run again.
    expect(sc.posted).toEqual([{ callId: "call_0", status: "error", content: INTERRUPTED_RESULT }]);
    expect(handle).not.toHaveBeenCalled();
    // Events were read from the saved sequence number, not from the start.
    const reads = (sc.api.events as ReturnType<typeof vi.fn>).mock.calls.slice(before).map((c) => c[1]);
    expect(reads[0]).toBe(2);
    // The turn's answer is shown once, and the chat is ready again.
    const answers = store.state.chatMsgs.filter((m) => m.who !== "You");
    expect(answers.length).toBe(1);
    expect(answers[0].text).toContain("could not be read");
    expect(store.state.chatStatus).toBe("ready");
    // The saved session now points past the finished turn, with nothing in flight.
    const after = parseSavedSession(JSON.parse(localStorage.getItem(KEY) ?? "null"));
    expect(after).toEqual({ v: 1, id: "s1-77aa", lastSeq: sc.log.length, inFlight: [] });
  });

  it("a saved entry that is not the saved shape is ignored after a reload", async () => {
    const sc = fakeSidecar();
    localStorage.setItem(KEY, JSON.stringify({ v: 1, id: "../x", lastSeq: 3, inFlight: [] }));
    const reloaded = provider(sc.api);
    await (store as unknown as { resumeSidecarSession(p: ChatProvider): Promise<void> }).resumeSidecarSession(reloaded);
    expect(sc.api.events).not.toHaveBeenCalled();
    expect(sc.posted).toEqual([]);
    expect(store.state.chatMsgs).toEqual([]);
  });
});
