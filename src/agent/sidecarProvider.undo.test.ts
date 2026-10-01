// HUP-S2.9 — a sidecar-hosted file tool that changed a file reaches the app as a file_change
// activity carrying its checkpoint, so the chat can show the change with Undo. Core never runs
// these calls and never posts a result for them.
import { describe, it, expect, vi } from "vitest";
import { createSidecarProvider, type SidecarSessionApi } from "./sidecarProvider";
import type { TurnActivityEvent } from "./harness";

type Ev = { seq: number; event: Record<string, unknown> };

function api(events: Ev[]) {
  let served = false;
  const a: SidecarSessionApi = {
    open: vi.fn(async () => "s4-cafe"),
    send: vi.fn(async () => {}),
    events: vi.fn(async (_id: string, after: number) => {
      if (served) return { events: [], lastSeq: after, busy: false };
      served = true;
      return { events, lastSeq: events[events.length - 1].seq, busy: false };
    }),
    toolResult: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
  };
  return a;
}

const call = (seq: number, id: string, name: string, host: string | null): Ev => ({
  seq,
  event: { type: "tool_call", step: 1, call: { id, name, arguments: "{}" }, host },
});
const result = (seq: number, id: string, status: string, content: string): Ev => ({
  seq,
  event: { type: "tool_result", step: 1, call_id: id, status, content },
});
const change = (s: number) => JSON.stringify({ ok: true, tool: "fs_write", paths: ["/w/notes.md"], checkpoint: { session: "s4-cafe", seq: s } });

describe("HUP-S2.9 sidecar provider file changes", () => {
  it("reports each successful sidecar file change once, with its checkpoint", async () => {
    const a = api([
      { seq: 1, event: { type: "step_start", step: 1 } },
      call(2, "w1", "fs_write", "sidecar"),
      result(3, "w1", "ok", change(1)),
      call(4, "w2", "fs_edit", "sidecar"),
      result(5, "w2", "error", change(2)), // only an ok result is a change, whatever its text
      call(6, "w3", "fs_delete", null),
      result(7, "w3", "denied", "declined: needs approval"),
      { seq: 8, event: { type: "final", content: "Saved." } },
      { seq: 9, event: { type: "done", outcome: "answered" } },
    ]);
    const seen: TurnActivityEvent[] = [];
    const onToolCall = vi.fn(async () => "ok");
    const p = createSidecarProvider(a, () => "p", () => []);
    const out = await p.send({
      messages: [{ role: "user", content: "write my notes" }],
      callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall, onActivity: (e) => seen.push(e) },
    });
    expect(out.content).toBe("Saved.");
    expect(seen.filter((e) => e.kind === "file_change")).toEqual([
      { kind: "file_change", change: { session: "s4-cafe", seq: 1, tool: "fs_write", paths: ["/w/notes.md"] } },
    ]);
    expect(onToolCall).not.toHaveBeenCalled();
    expect(a.toolResult).not.toHaveBeenCalled();
  });

  it("ignores a core-hosted tool that happens to return checkpoint-shaped text", async () => {
    const a = api([
      call(1, "c1", "fs_write", "core"),
      result(2, "c1", "ok", change(1)),
      { seq: 3, event: { type: "done", outcome: "answered" } },
    ]);
    const seen: TurnActivityEvent[] = [];
    const p = createSidecarProvider(a, () => "p", () => []);
    await p.send({
      messages: [{ role: "user", content: "x" }],
      callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => change(1)), onActivity: (e) => seen.push(e) },
    });
    expect(seen.some((e) => e.kind === "file_change")).toBe(false);
  });
});
