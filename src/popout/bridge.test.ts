// HUP-S5.4 — the typed message bridge between the main window and a pop-out. Every message is
// versioned and validated on receipt; anything malformed is dropped, never half-applied.
import { describe, it, expect, vi } from "vitest";
import { parseToMain, parseToPopout, createMainEnd, createPopoutEnd, POPOUT_EVENT, type BridgeTransport } from "./bridge";
import { buildMonitorSnapshot } from "./monitorSnapshot";
import { IDLE_ACTIVITY } from "../shell/slices/turnActivity";

const IDLE_SNAPSHOT_FOR_TESTS = buildMonitorSnapshot({
  activity: IDLE_ACTIVITY,
  providerKind: "local",
  providerLabel: "local model · llama-server · agentic",
  modelLabel: "local model",
  modelId: null,
  tier: null,
  localCtxTokens: 8192,
  now: 0,
});

/** An in-memory bus: every window's transport delivers to listeners registered under its label. */
function bus() {
  const listeners = new Map<string, ((p: unknown) => void)[]>();
  const sent: { from: string; to: string; payload: unknown }[] = [];
  const transport = (self: string): BridgeTransport => ({
    async send(to, payload) {
      sent.push({ from: self, to, payload });
      for (const f of listeners.get(to) ?? []) f(JSON.parse(JSON.stringify(payload)));
    },
    async listen(handler) {
      const arr = listeners.get(self) ?? [];
      arr.push(handler);
      listeners.set(self, arr);
      return () => listeners.set(self, (listeners.get(self) ?? []).filter((f) => f !== handler));
    },
  });
  return { transport, sent };
}

describe("HUP-S5.4 bridge message validation", () => {
  it("accepts the three popout→main messages", () => {
    expect(parseToMain({ v: 1, type: "popout.ready", kind: "monitor" })).toEqual({ v: 1, type: "popout.ready", kind: "monitor" });
    expect(parseToMain({ v: 1, type: "monitor.stop" })).toEqual({ v: 1, type: "monitor.stop" });
  });

  it("drops wrong versions, unknown types, unknown kinds and non-objects", () => {
    for (const raw of [
      null, undefined, "monitor.stop", 7, [], {},
      { v: 2, type: "monitor.stop" },
      { type: "monitor.stop" },
      { v: 1, type: "monitor.start" },
      { v: 1, type: "popout.ready", kind: "shell" },
      { v: 1, type: "popout.ready" },
      { v: "1", type: "monitor.stop" },
    ]) {
      expect(parseToMain(raw)).toBeNull();
    }
  });

  it("accepts a well-formed snapshot and drops a malformed one", () => {
    const ok = { v: 1, type: "monitor.snapshot", snapshot: IDLE_SNAPSHOT_FOR_TESTS };
    expect(parseToPopout(ok)).toEqual(ok);
    expect(parseToPopout({ v: 1, type: "monitor.snapshot" })).toBeNull();
    expect(parseToPopout({ v: 1, type: "monitor.snapshot", snapshot: { ...IDLE_SNAPSHOT_FOR_TESTS, turn: null } })).toBeNull();
    expect(parseToPopout({ v: 1, type: "monitor.snapshot", snapshot: { ...IDLE_SNAPSHOT_FOR_TESTS, context: { ...IDLE_SNAPSHOT_FOR_TESTS.context, usedTokens: "lots" } } })).toBeNull();
    expect(parseToPopout({ v: 1, type: "monitor.stop" })).toBeNull();
  });
});

describe("HUP-S5.4 bridge ends", () => {
  it("a pop-out announcing ready gets the current snapshot; stop reaches the main end", async () => {
    const b = bus();
    const onStop = vi.fn();
    const main = await createMainEnd(b.transport("main"), {
      onReady: (kind) => { if (kind === "monitor") void main.sendSnapshot(IDLE_SNAPSHOT_FOR_TESTS); },
      onStop,
    });
    const got: unknown[] = [];
    const pop = await createPopoutEnd(b.transport("popout-monitor"), "monitor", (s) => got.push(s));
    await pop.ready();
    expect(got).toEqual([IDLE_SNAPSHOT_FOR_TESTS]);
    await pop.stop();
    expect(onStop).toHaveBeenCalledTimes(1);
    expect(b.sent.every((m) => m.to === "main" || m.to === "popout-monitor")).toBe(true);
    main.close();
    pop.close();
  });

  it("the main end ignores garbage on the channel", async () => {
    const b = bus();
    const onStop = vi.fn();
    const onReady = vi.fn();
    const main = await createMainEnd(b.transport("main"), { onReady, onStop });
    const rogue = b.transport("popout-monitor");
    await rogue.send("main", { v: 1, type: "monitor.stopp" });
    await rogue.send("main", { v: 9, type: "monitor.stop" });
    await rogue.send("main", "monitor.stop");
    expect(onStop).not.toHaveBeenCalled();
    expect(onReady).not.toHaveBeenCalled();
    main.close();
  });

  it("after close, an end no longer receives", async () => {
    const b = bus();
    const onStop = vi.fn();
    const main = await createMainEnd(b.transport("main"), { onReady: vi.fn(), onStop });
    main.close();
    await b.transport("popout-monitor").send("main", { v: 1, type: "monitor.stop" });
    expect(onStop).not.toHaveBeenCalled();
  });

  it("uses one event name for the channel", () => {
    expect(POPOUT_EVENT).toMatch(/^[a-z0-9:_/-]+$/);
  });
});
