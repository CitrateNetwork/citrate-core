// #691: the sidecar provider steers the running turn in place; #693 review: a branched
// conversation opens its session with the earlier turns as history, never the system prompt.
import { describe, it, expect, vi } from "vitest";
import { BRANCH_HISTORY_REFUSED, createSidecarProvider, historyRefused, type SidecarSessionApi } from "./sidecarProvider";
import type { TurnActivityEvent } from "./harness";

type Page = { events: { seq: number; event: Record<string, unknown> }[]; lastSeq: number; busy: boolean };

/** A session api whose events wait until `release()` is called, so a turn stays live. */
function liveApi(pages: Page[]) {
  let i = 0;
  let release: () => void = () => undefined;
  const gate = new Promise<void>((r) => (release = r));
  const api: SidecarSessionApi = {
    open: vi.fn(async () => "s1"),
    send: vi.fn(async () => undefined),
    events: vi.fn(async () => {
      if (i === 0) await gate;
      return pages[Math.min(i++, pages.length - 1)];
    }),
    toolResult: vi.fn(async () => undefined),
    stop: vi.fn(async () => undefined),
    steer: vi.fn(async () => 1),
  };
  return { api, release };
}

const answered: Page = {
  events: [
    { seq: 1, event: { type: "step_start", step: 1 } },
    { seq: 2, event: { type: "steered", step: 2, text: "also skip Fridays" } },
    { seq: 3, event: { type: "step_start", step: 2 } },
    { seq: 4, event: { type: "final", content: "Weeknights only." } },
    { seq: 5, event: { type: "done", outcome: "answered" } },
  ],
  lastSeq: 5,
  busy: false,
};

const cb = (seen: TurnActivityEvent[]) => ({ onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "r"), onActivity: (e: TurnActivityEvent) => seen.push(e) });

describe("sidecar provider: steering the running turn", () => {
  it("posts the steer to the live session and reports where Hermes took it", async () => {
    const { api, release } = liveApi([answered]);
    const p = createSidecarProvider(api, () => "p", () => []);
    const seen: TurnActivityEvent[] = [];
    const turn = p.send({ messages: [{ role: "user", content: "nights?" }], callbacks: cb(seen) });
    await vi.waitFor(() => expect(api.events).toHaveBeenCalled());
    expect(await p.steer!("  also skip Fridays ")).toEqual({ ok: true, waiting: 1 });
    expect(api.steer).toHaveBeenCalledWith("s1", "also skip Fridays");
    release();
    await turn;
    expect(seen).toContainEqual({ kind: "steered", step: 2, text: "also skip Fridays" });
    // Between turns there is nothing to steer.
    expect(await p.steer!("x")).toEqual({ ok: false, reason: "no run is live" });
  });

  it("names why a steer was not taken: run finishing (409), an older Hermes (404), no support", async () => {
    const { api, release } = liveApi([answered]);
    const p = createSidecarProvider(api, () => "p", () => []);
    const turn = p.send({ messages: [{ role: "user", content: "x" }], callbacks: cb([]) });
    await vi.waitFor(() => expect(api.events).toHaveBeenCalled());
    (api.steer as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error("hermes control returned 409: no turn"));
    expect(await p.steer!("a")).toEqual({ ok: false, reason: "the run was already finishing" });
    (api.steer as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error("hermes control returned 404: "));
    expect(await p.steer!("a")).toEqual({ ok: false, reason: "the bundled Hermes is too old to take messages mid-run" });
    expect(await p.steer!("   ")).toEqual({ ok: false, reason: "the message is empty" });
    release();
    await turn;
    const { api: old } = liveApi([answered]);
    delete old.steer;
    expect((await createSidecarProvider(old, () => "p", () => []).steer!("a")).ok).toBe(false);
  });
});

describe("sidecar provider: a branch opens with its earlier turns", () => {
  const seed = [
    { role: "user" as const, content: "Network status" },
    { role: "assistant" as const, content: "Synced at block 10." },
  ];

  it("passes them to the session open as history, and the prompt stays as composed", async () => {
    const { api, release } = liveApi([answered]);
    const p = createSidecarProvider(api, () => "base prompt", () => [], () => null, { history: () => seed });
    release();
    await p.send({ messages: [{ role: "user", content: "and peers?" }], callbacks: cb([]) });
    expect(api.open).toHaveBeenCalledWith("base prompt", "[]", undefined, seed);
  });

  it("an older Hermes that refuses the history key: the branch opens without it and says so", async () => {
    const { api, release } = liveApi([answered]);
    (api.open as ReturnType<typeof vi.fn>)
      .mockRejectedValueOnce(new Error("hermes control returned 400: unknown field `history`"))
      .mockResolvedValueOnce("s2");
    const seen: TurnActivityEvent[] = [];
    const p = createSidecarProvider(api, () => "base prompt", () => [], () => null, { history: () => seed });
    release();
    await p.send({ messages: [{ role: "user", content: "and peers?" }], callbacks: cb(seen) });
    expect(api.open).toHaveBeenLastCalledWith("base prompt", "[]", undefined);
    expect(seen).toContainEqual({ kind: "notice", text: BRANCH_HISTORY_REFUSED });
  });

  it("any other open failure is not mistaken for a refused history", () => {
    expect(historyRefused(new Error("hermes control returned 400: unknown field `history`"))).toBe(true);
    expect(historyRefused(new Error("hermes control returned 503: emergency stop engaged"))).toBe(false);
  });
});
