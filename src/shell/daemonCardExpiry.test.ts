// HUP-S10.3 hardening — an approval card a daemon run raised closes when that run ends (timed out,
// stopped or paused). A later click must not act outside the run that asked.
import { describe, it, expect, afterEach, beforeEach } from "vitest";
import { store } from "./store";
import type { ToolCall } from "../agent/harness";

const call = (name: string, args: Record<string, unknown> = {}): ToolCall => ({ id: "c1", name, arguments: JSON.stringify(args) });
const HIC = { hic: "required" as const, hicReason: 'the scheduled daemon "Digest" proposed this while you were not watching' };
const ticks = async (n = 20) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

beforeEach(() => {
  store.setState({ chatMsgs: [], queue: [], cerPhase: "review", walletReview: null });
});
afterEach(() => {
  store.setState({ queue: [], walletReview: null });
});

describe("daemon approval cards end with their run", () => {
  it("a card is withdrawn when the run's signal aborts, and the tool never runs", async () => {
    const ac = new AbortController();
    const out = store.handleTool(call("journal_append", { entry: "x" }), "daemon-run", () => undefined, HIC, ac.signal);
    await ticks();
    expect(store.state.queue.length).toBe(1);
    ac.abort();
    const result = await out;
    expect(store.state.queue.length).toBe(0);
    expect(result).toMatch(/run ended|expired/i);
    expect(result).toMatch(/nothing was done/);
  });

  it("a card for a run that already ended is never shown", async () => {
    const ac = new AbortController();
    ac.abort();
    const result = await store.handleTool(call("journal_append", { entry: "x" }), "daemon-run", () => undefined, HIC, ac.signal);
    expect(store.state.queue.length).toBe(0);
    expect(result).toMatch(/nothing was done/);
  });

  it("only the run's own card goes; other cards stay", async () => {
    const other = store.requestSig({ origin: "x", requester: "y", title: "keep me", rows: [], cost: "", sponsor: "", sponsorColor: "" });
    const ac = new AbortController();
    const mine = store.requestSig({ origin: "x", requester: "y", title: "daemon card", rows: [], cost: "", sponsor: "", sponsorColor: "" }, ac.signal);
    expect(store.state.queue.map((q) => q.title)).toEqual(["keep me", "daemon card"]);
    ac.abort();
    await expect(mine).resolves.toBe("expired");
    expect(store.state.queue.map((q) => q.title)).toEqual(["keep me"]);
    store.finishCer("declined");
    await expect(other).resolves.toBe("declined");
  });

  it("a card already being approved is not pulled out from under the member", async () => {
    const ac = new AbortController();
    const mine = store.requestSig({ origin: "x", requester: "y", title: "daemon card", rows: [], cost: "", sponsor: "", sponsorColor: "", chainless: true }, ac.signal);
    store.setState({ cerPhase: "busy" });
    ac.abort();
    expect(store.state.queue.length).toBe(1);
    store.finishCer("approved");
    await expect(mine).resolves.toBe("approved");
  });
});
