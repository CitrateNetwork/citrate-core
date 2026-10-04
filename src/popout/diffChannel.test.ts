// HUP-S5.4 — the Code and diff pop-out's request channel: the pop-out asks, the main window checks
// every request again and answers from Rust; the pop-out checks every answer before rendering.
import { describe, expect, it, vi } from "vitest";
import type { BridgeTransport } from "./bridge";
import { checkArgs, checkResult, createDiffClient, createDiffHost, parseFocus, parseRequest, type DiffOps } from "./diffChannel";
import { diffOps, resetDiffFocus, focusDiff } from "./diffHost";

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

const LIST = { session: "s3-ab", enabled: true, steps: [{ seq: 2, status: "committed", paths: ["a.txt"], root: "/w" }], note: null };
const DIFF = { ok: true, session: "s3-ab", seq: 2, status: "committed", files: [{ path: "a.txt", before: { kind: "text", text: "a\n" }, after: { kind: "text", text: "b\n" } }], kind: null, reason: null };

function ops(over: Partial<DiffOps> = {}): DiffOps {
  return {
    initial: vi.fn(async () => ({ session: "s3-ab", seq: 2 })),
    steps: vi.fn(async () => LIST),
    diff: vi.fn(async () => DIFF),
    ...over,
  };
}

describe("HUP-S5.4 diff channel validation", () => {
  it("checks every op's arguments", () => {
    expect(checkArgs("initial", {})).toEqual({});
    expect(checkArgs("steps", { session: "s3-ab" })).toEqual({ session: "s3-ab" });
    expect(checkArgs("steps", { session: "../x" })).toBeNull();
    expect(checkArgs("diff", { session: "s3-ab", seq: 2 })).toEqual({ session: "s3-ab", seq: 2 });
    for (const seq of [0, -1, 1.5, "2", null]) expect(checkArgs("diff", { session: "s3-ab", seq })).toBeNull();
    expect(checkArgs("steps", null)).toBeNull();
  });

  it("drops traffic that is not a diff request and refuses unknown ops", () => {
    expect(parseRequest({ v: 1, type: "contract.request", id: "a", op: "source" })).toBeNull();
    expect(parseRequest({ v: 1, type: "diff.request", id: "a", op: "undo" })).toBeNull();
    expect(parseRequest({ v: 2, type: "diff.request", id: "a", op: "diff" })).toBeNull();
    expect(parseRequest({ v: 1, type: "diff.request", id: "has space", op: "diff" })).toBeNull();
    expect(parseFocus({ v: 1, type: "diff.focus", session: "s3-ab", seq: null })).not.toBeNull();
    expect(parseFocus({ v: 1, type: "diff.focus", session: "s3-ab", seq: 0 })).toBeNull();
  });

  it("checks answers before the pop-out renders them", () => {
    expect(checkResult("initial", null)).toEqual({ ok: true, value: null });
    expect(checkResult("initial", { session: "bad id", seq: null }).ok).toBe(false);
    expect(checkResult("steps", LIST)).toEqual({ ok: true, value: LIST });
    expect(checkResult("steps", { ...LIST, steps: [{ seq: "2" }] }).ok).toBe(false);
    expect(checkResult("diff", DIFF).ok).toBe(true);
    expect(checkResult("diff", { ...DIFF, files: [{ path: "a", before: { kind: "html", html: "<b>" }, after: { kind: "absent" } }] }).ok).toBe(false);
  });
});

describe("HUP-S5.4 diff channel round trip", () => {
  it("the pop-out asks and the main window answers through its ops", async () => {
    const b = bus();
    const o = ops();
    const host = await createDiffHost(b.transport("main"), o);
    const client = await createDiffClient(b.transport("popout-diff"));
    expect(await client.call("initial", {})).toEqual({ session: "s3-ab", seq: 2 });
    expect(await client.call("steps", { session: "s3-ab" })).toEqual(LIST);
    expect((await client.call("diff", { session: "s3-ab", seq: 2 })).files[0].path).toBe("a.txt");
    expect(o.diff).toHaveBeenCalledWith({ session: "s3-ab", seq: 2 });
    expect(b.sent.every((s) => (s.from === "popout-diff" ? s.to === "main" : s.to === "popout-diff"))).toBe(true);
    host.close();
    client.close();
  });

  it("a malformed request is refused without running anything, and errors come back as errors", async () => {
    const b = bus();
    const o = ops({ diff: vi.fn(async () => { throw new Error("hermes is not running"); }) });
    const host = await createDiffHost(b.transport("main"), o);
    const client = await createDiffClient(b.transport("popout-diff"));
    await expect(client.call("diff", { session: "s3-ab", seq: 0 } as never)).rejects.toThrow(/malformed/);
    expect(o.diff).not.toHaveBeenCalled();
    await expect(client.call("diff", { session: "s3-ab", seq: 2 })).rejects.toThrow(/hermes is not running/);
    host.close();
    client.close();
  });

  it("an answer the pop-out cannot read is an error, not a render", async () => {
    const b = bus();
    const host = await createDiffHost(b.transport("main"), ops({ steps: vi.fn(async () => ({ session: "s3-ab", enabled: "yes" }) as never) }));
    const client = await createDiffClient(b.transport("popout-diff"));
    await expect(client.call("steps", { session: "s3-ab" })).rejects.toThrow(/could not be read/);
    host.close();
    client.close();
  });

  it("focus tells an open pop-out which change to show", async () => {
    const b = bus();
    const host = await createDiffHost(b.transport("main"), ops());
    const onFocus = vi.fn();
    const client = await createDiffClient(b.transport("popout-diff"), onFocus);
    await host.focus("s4-cafe", 3);
    expect(onFocus).toHaveBeenCalledWith({ session: "s4-cafe", seq: 3 });
    host.close();
    client.close();
  });
});

describe("HUP-S5.4 diff host ops", () => {
  it("starts on the change a Diff button named, else the latest agent session", async () => {
    resetDiffFocus();
    const api = { checkpoints: vi.fn(async () => LIST), checkpointDiff: vi.fn(async () => DIFF) };
    let current: string | null = null;
    const o = diffOps(api as never, () => current);
    expect(await o.initial({})).toBeNull();
    current = "s9-beef";
    expect(await o.initial({})).toEqual({ session: "s9-beef", seq: null });
    await focusDiff("s3-ab", 2);
    expect(await o.initial({})).toEqual({ session: "s3-ab", seq: 2 });
    // A focus is used once.
    expect(await o.initial({})).toEqual({ session: "s9-beef", seq: null });
    await o.diff({ session: "s3-ab", seq: 2 });
    expect(api.checkpointDiff).toHaveBeenCalledWith("s3-ab", 2);
    await o.steps({ session: "s3-ab" });
    expect(api.checkpoints).toHaveBeenCalledWith("s3-ab");
  });
});
