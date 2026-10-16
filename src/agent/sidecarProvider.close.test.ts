// A chat session the provider leaves (its turn was stopped, so its stop switch stays on) is closed
// in the sidecar, so repeated Stop presses cannot fill the sidecar's session table.
import { describe, it, expect, vi } from "vitest";
import { createSidecarProvider, type SidecarSessionApi } from "./sidecarProvider";

type Page = { events: { seq: number; event: Record<string, unknown> }[]; lastSeq: number; busy: boolean };

function api() {
  let n = 0;
  const pages = new Map<string, Page[]>();
  const closed: string[] = [];
  const a: SidecarSessionApi = {
    open: vi.fn(async () => {
      const id = `s${++n}`;
      pages.set(id, [
        { events: [{ seq: 1, event: { type: "done", outcome: "stopped" } }], lastSeq: 1, busy: false },
      ]);
      return id;
    }),
    send: vi.fn(async () => {}),
    events: vi.fn(async (id: string, after: number) => pages.get(id)?.shift() ?? { events: [], lastSeq: after, busy: false }),
    toolResult: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
    close: vi.fn(async (id: string) => {
      closed.push(id);
    }),
  };
  return { a, closed };
}

const cb = () => ({ onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "ok") });

describe("sessions the chat provider leaves are closed", () => {
  it("a stopped session is closed when the provider moves to a fresh one", async () => {
    const { a, closed } = api();
    const p = createSidecarProvider(a, () => "p", () => []);
    for (let i = 0; i < 10; i++) {
      await p.send({ messages: [{ role: "user", content: `turn ${i}` }], callbacks: cb() }).catch(() => undefined);
    }
    expect(a.open).toHaveBeenCalledTimes(10);
    expect(closed).toEqual(["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10"]);
  });

  it("a stop pressed during the turn also ends with the session closed", async () => {
    const { a, closed } = api();
    const p = createSidecarProvider(a, () => "p", () => []);
    const ac = new AbortController();
    ac.abort();
    await p.send({ messages: [{ role: "user", content: "x" }], callbacks: cb(), signal: ac.signal }).catch(() => undefined);
    const ac2 = new AbortController();
    const turn = p.send({ messages: [{ role: "user", content: "y" }], callbacks: cb(), signal: ac2.signal }).catch(() => undefined);
    ac2.abort();
    await turn;
    await new Promise((r) => setTimeout(r, 10));
    expect(closed.length).toBe((a.open as ReturnType<typeof vi.fn>).mock.calls.length);
  });
});
