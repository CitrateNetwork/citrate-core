// The sidecar session's life cycle as core sees it. The sidecar keeps at most a few sessions and,
// when its table is full, replaces the idle one used least recently: a later call on the replaced
// session answers 404, and the provider opens a fresh session once and carries on. A session the
// provider stops using (it was stopped) is closed, so it never sits in the sidecar's table.
import { describe, it, expect, vi } from "vitest";
import { createSidecarProvider, type SidecarSessionApi } from "./sidecarProvider";

type Ev = { seq: number; event: Record<string, unknown> };
type Page = { events: Ev[]; lastSeq: number; busy: boolean };

const answered = (content: string): Page => ({
  events: [
    { seq: 1, event: { type: "final", content } },
    { seq: 2, event: { type: "done", outcome: "answered" } },
  ],
  lastSeq: 2,
  busy: false,
});

const stoppedPage: Page = {
  events: [{ seq: 1, event: { type: "done", outcome: "stopped" } }],
  lastSeq: 1,
  busy: false,
};

function callbacks() {
  return { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "ok") };
}

const turn = (content: string) => ({ messages: [{ role: "user" as const, content }], callbacks: callbacks() });

describe("sidecar session life cycle", () => {
  it("reopens a session the sidecar replaced (404) once and sends the turn there", async () => {
    let opened = 0;
    const pages: Record<string, Page[]> = { s1: [answered("first")], s2: [answered("second")] };
    const api: SidecarSessionApi = {
      open: vi.fn(async () => `s${++opened}`),
      send: vi.fn(async (id: string) => {
        // The sidecar replaced s1 after the first turn.
        if (id === "s1" && opened === 1 && (api.send as ReturnType<typeof vi.fn>).mock.calls.length > 1) {
          throw "hermes control returned 404: no such session";
        }
      }),
      events: vi.fn(async (id: string, after: number) => pages[id]?.shift() ?? { events: [], lastSeq: after, busy: false }),
      toolResult: vi.fn(async () => {}),
      stop: vi.fn(async () => {}),
      close: vi.fn(async () => {}),
    };
    const p = createSidecarProvider(api, () => "BASE", () => []);
    expect((await p.send(turn("one"))).content).toBe("first");
    expect((await p.send(turn("two"))).content).toBe("second");
    expect(api.open).toHaveBeenCalledTimes(2);
    expect(api.send).toHaveBeenLastCalledWith("s2", "two");
  });

  it("does not retry a send that failed for another reason", async () => {
    const api: SidecarSessionApi = {
      open: vi.fn(async () => "s1"),
      send: vi.fn(async () => {
        throw new Error("hermes control returned 500: the model endpoint failed");
      }),
      events: vi.fn(async (_id: string, after: number) => ({ events: [], lastSeq: after, busy: false })),
      toolResult: vi.fn(async () => {}),
      stop: vi.fn(async () => {}),
      close: vi.fn(async () => {}),
    };
    const p = createSidecarProvider(api, () => "BASE", () => []);
    await expect(p.send(turn("one"))).rejects.toThrow(/500/);
    expect(api.open).toHaveBeenCalledTimes(1);
    expect(api.send).toHaveBeenCalledTimes(1);
  });

  it("a reopened session that is also gone is reported, not retried again", async () => {
    let opened = 0;
    const api: SidecarSessionApi = {
      open: vi.fn(async () => `s${++opened}`),
      send: vi.fn(async () => {
        throw "hermes control returned 404: no such session";
      }),
      events: vi.fn(async (_id: string, after: number) => ({ events: [], lastSeq: after, busy: false })),
      toolResult: vi.fn(async () => {}),
      stop: vi.fn(async () => {}),
      close: vi.fn(async () => {}),
    };
    const p = createSidecarProvider(api, () => "BASE", () => []);
    await expect(p.send(turn("one"))).rejects.toThrow(/404/);
    expect(api.open).toHaveBeenCalledTimes(2);
  });

  it("closes a session that was stopped, after its events are drained, and opens a fresh one next", async () => {
    let opened = 0;
    const pages: Record<string, Page[]> = { s1: [stoppedPage], s2: [answered("fresh")] };
    const api: SidecarSessionApi = {
      open: vi.fn(async () => `s${++opened}`),
      send: vi.fn(async () => {}),
      events: vi.fn(async (id: string, after: number) => pages[id]?.shift() ?? { events: [], lastSeq: after, busy: false }),
      toolResult: vi.fn(async () => {}),
      stop: vi.fn(async () => {}),
      close: vi.fn(async () => {}),
    };
    const p = createSidecarProvider(api, () => "BASE", () => []);
    await expect(p.send(turn("one"))).rejects.toThrow(/stopped/);
    expect(api.close).toHaveBeenCalledWith("s1");
    expect((await p.send(turn("two"))).content).toBe("fresh");
    expect(api.close).toHaveBeenCalledTimes(1);
  });

  it("a session in use is never closed", async () => {
    const api: SidecarSessionApi = {
      open: vi.fn(async () => "s1"),
      send: vi.fn(async () => {}),
      events: vi.fn(async () => answered("ok")),
      toolResult: vi.fn(async () => {}),
      stop: vi.fn(async () => {}),
      close: vi.fn(async () => {}),
    };
    const p = createSidecarProvider(api, () => "BASE", () => []);
    await p.send(turn("one"));
    expect(api.close).not.toHaveBeenCalled();
  });
});
