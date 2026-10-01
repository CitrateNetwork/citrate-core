// HUP-S1.1c — the webview becomes a VIEW over the sidecar-owned loop (ADR loop-in-sidecar).
// The provider opens a session, sends the turn, long-polls events, and runs every core-hosted
// tool call through the store's gated handler (the same approval gates as today), posting the
// result back. It never decides an approval and never talks to the model directly.
import { describe, it, expect, vi } from "vitest";
import { createSidecarProvider, type SidecarSessionApi } from "./sidecarProvider";

type Page = { events: { seq: number; event: Record<string, unknown> }[]; lastSeq: number; busy: boolean };

function fakeApi(pages: Page[]) {
  const q = pages.slice();
  const api: SidecarSessionApi & { calls: string[] } = {
    calls: [],
    open: vi.fn(async () => { api.calls.push("open"); return "s1-abc"; }),
    send: vi.fn(async (_id: string, text: string) => { api.calls.push("send:" + text); }),
    events: vi.fn(async (_id: string, after: number) => { api.calls.push("events:" + after); return q.shift() ?? { events: [], lastSeq: after, busy: false }; }),
    toolResult: vi.fn(async (_id: string, callId: string, status: string, content: string) => { api.calls.push(`result:${callId}:${status}:${content}`); }),
    stop: vi.fn(async () => { api.calls.push("stop"); }),
  };
  return api;
}

const cb = () => ({ onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => '{"height":6310}') });

describe("HUP-S1.1c sidecar chat provider", () => {
  it("streams a plain answer from the session's events", async () => {
    const api = fakeApi([
      { events: [{ seq: 1, event: { type: "step_start", step: 1 } }], lastSeq: 1, busy: true },
      { events: [{ seq: 2, event: { type: "final", content: "Your node is validating." } }, { seq: 3, event: { type: "done", outcome: "answered" } }], lastSeq: 3, busy: false },
    ]);
    const p = createSidecarProvider(api, () => "You are Hermes.", () => []);
    const c = cb();
    const out = await p.send({ messages: [{ role: "user", content: "status?" }], callbacks: c });
    expect(out.content).toBe("Your node is validating.");
    expect(c.onToken).toHaveBeenCalledWith("Your node is validating.");
    expect(c.onStatus).toHaveBeenLastCalledWith("done");
    expect(api.calls.slice(0, 2)).toEqual(["open", "send:status?"]);
    expect(api.calls).toContain("events:1");
  });

  it("runs a core tool call through the gated handler and posts the result back", async () => {
    const api = fakeApi([
      { events: [{ seq: 1, event: { type: "step_start", step: 1 } }, { seq: 2, event: { type: "tool_call", step: 1, host: "core", call: { id: "c1", name: "node_status", arguments: "{}" } } }], lastSeq: 2, busy: true },
      { events: [{ seq: 3, event: { type: "final", content: "Height 6,310." } }, { seq: 4, event: { type: "done", outcome: "answered" } }], lastSeq: 4, busy: false },
    ]);
    const p = createSidecarProvider(api, () => "p", () => []);
    const c = cb();
    await p.send({ messages: [{ role: "user", content: "height?" }], callbacks: c });
    expect(c.onToolCall).toHaveBeenCalledWith(expect.objectContaining({ id: "c1", name: "node_status" }));
    expect(api.calls).toContain('result:c1:ok:{"height":6310}');
  });

  it("a declined approval is reported to the loop as denied", async () => {
    const api = fakeApi([
      { events: [{ seq: 1, event: { type: "tool_call", step: 1, host: "core", call: { id: "c9", name: "group_invite", arguments: "{}" } } }], lastSeq: 1, busy: true },
      { events: [{ seq: 2, event: { type: "final", content: "ok" } }, { seq: 3, event: { type: "done", outcome: "answered" } }], lastSeq: 3, busy: false },
    ]);
    const p = createSidecarProvider(api, () => "p", () => []);
    const c = { ...cb(), onToolCall: vi.fn(async () => "the member declined minting an invite link; no link exists.") };
    await p.send({ messages: [{ role: "user", content: "invite" }], callbacks: c });
    expect(api.calls.find((x) => x.startsWith("result:c9:"))).toMatch(/^result:c9:denied:/);
  });

  it("a failed turn rejects with the loop's error so the chat shows Retry", async () => {
    const api = fakeApi([{ events: [{ seq: 1, event: { type: "error", message: "model transport error: could not connect" } }, { seq: 2, event: { type: "done", outcome: "failed" } }], lastSeq: 2, busy: false }]);
    const p = createSidecarProvider(api, () => "p", () => []);
    await expect(p.send({ messages: [{ role: "user", content: "x" }], callbacks: cb() })).rejects.toThrow(/could not connect/);
  });

  it("reuses one session across turns", async () => {
    const done = (s: number): Page => ({ events: [{ seq: s, event: { type: "final", content: "k" } }, { seq: s + 1, event: { type: "done", outcome: "answered" } }], lastSeq: s + 1, busy: false });
    const api = fakeApi([done(1), done(3)]);
    const p = createSidecarProvider(api, () => "p", () => []);
    await p.send({ messages: [{ role: "user", content: "a" }], callbacks: cb() });
    await p.send({ messages: [{ role: "user", content: "b" }], callbacks: cb() });
    expect(api.calls.filter((x) => x === "open")).toHaveLength(1);
    expect(api.calls).toContain("events:2");
  });
});
