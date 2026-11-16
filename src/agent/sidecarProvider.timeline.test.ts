// FL design board 01: the sidecar provider reports every tool call as a step (sidecar-hosted and
// refused ones too) and the time each event page arrived, so the run timeline never guesses.
import { describe, it, expect, vi } from "vitest";
import { createSidecarProvider, type SidecarSessionApi } from "./sidecarProvider";
import type { TurnActivityEvent } from "./harness";

function api(pages: { events: { seq: number; event: Record<string, unknown> }[]; lastSeq: number; busy: boolean }[]): SidecarSessionApi {
  let i = 0;
  return {
    open: vi.fn(async () => "s1"),
    send: vi.fn(async () => undefined),
    events: vi.fn(async () => pages[Math.min(i++, pages.length - 1)]),
    toolResult: vi.fn(async () => undefined),
    stop: vi.fn(async () => undefined),
  };
}

describe("sidecar provider: run timeline events", () => {
  it("reports sidecar-hosted and refused calls with their results, and each page's time", async () => {
    const a = api([
      {
        events: [
          { seq: 1, event: { type: "step_start", step: 1 } },
          { seq: 2, event: { type: "tool_call", step: 1, host: "sidecar", call: { id: "call_0", name: "mcp__fs__read", arguments: "{}" } } },
        ],
        lastSeq: 2,
        busy: true,
      },
      {
        events: [
          { seq: 3, event: { type: "tool_result", step: 1, call_id: "call_0", status: "ok", content: "file text" } },
          { seq: 4, event: { type: "tool_call", step: 1, host: null, call: { id: "call_1", name: "contract_deploy", arguments: "{}" } } },
          { seq: 5, event: { type: "tool_result", step: 1, call_id: "call_1", status: "denied", content: "needs your approval, which this host cannot ask for" } },
          { seq: 6, event: { type: "final", content: "done" } },
          { seq: 7, event: { type: "done", outcome: "answered" } },
        ],
        lastSeq: 7,
        busy: false,
      },
    ]);
    const seen: TurnActivityEvent[] = [];
    const p = createSidecarProvider(a, () => "p", () => []);
    await p.send({ messages: [{ role: "user", content: "hi" }], callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "r"), onActivity: (e) => seen.push(e) } });
    const kinds = seen.map((e) => e.kind);
    expect(kinds.filter((k) => k === "page")).toHaveLength(2);
    expect(seen).toContainEqual({ kind: "tool_start", callId: "call_0", tool: "mcp__fs__read", host: "sidecar" });
    expect(seen).toContainEqual({ kind: "tool_output", callId: "call_0", tool: "mcp__fs__read", status: "ok", content: "file text" });
    expect(seen).toContainEqual({ kind: "tool_start", callId: "call_1", tool: "contract_deploy", host: "refused" });
    expect(seen).toContainEqual({ kind: "tool_end", callId: "call_1", tool: "contract_deploy", status: "denied", content: "needs your approval, which this host cannot ask for" });
    // A refused call is never posted back as a core result (core never ran it).
    expect(a.toolResult).not.toHaveBeenCalled();
    // Each page's time comes before that page's events.
    expect(kinds.indexOf("page")).toBe(0);
  });

  it("an empty page reports no page time", async () => {
    const a = api([
      { events: [], lastSeq: 0, busy: true },
      { events: [{ seq: 1, event: { type: "final", content: "x" } }, { seq: 2, event: { type: "done", outcome: "answered" } }], lastSeq: 2, busy: false },
    ]);
    const seen: TurnActivityEvent[] = [];
    await createSidecarProvider(a, () => "p", () => []).send({
      messages: [{ role: "user", content: "hi" }],
      callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "r"), onActivity: (e) => seen.push(e) },
    });
    expect(seen.filter((e) => e.kind === "page")).toHaveLength(1);
  });
});
