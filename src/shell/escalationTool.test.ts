// HUP-S1.5 — escalate_plan through the REAL store.handleTool: within budget it shows a price notice
// and runs without a card (HIC-2); over budget or with a hic:"required" call it stops at the member's
// approval (HIC-1), and a decline sends nothing. The bridge is spied at its boundary.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import type { ToolCall } from "../agent/harness";
import type { EscalationQuote, EscalationRun } from "../bridge/domains";

const call: ToolCall = { id: "c1", name: "escalate_plan", arguments: JSON.stringify({ question: "Plan the mint page" }) };
const noop = () => {};

const EP = { id: "ep-1", label: "Planner", baseUrl: "https://api.example.com/v1", model: "big", inputMicrosPerMtok: 1, outputMicrosPerMtok: 1, destination: "Planner · api.example.com" };
const quote = (withinBudget: boolean): EscalationQuote => ({
  quoteId: "q-1",
  endpointId: "ep-1",
  destination: EP.destination,
  model: "big",
  costMicros: 5000,
  costLabel: "$0.005",
  withinBudget,
  remainingMicros: withinBudget ? 100_000 : 0,
  capMicros: withinBudget ? 200_000 : 0,
  maxTokens: 2048,
  promptBytes: 18,
  expiresMs: 0,
});
const run = (mode: "budget" | "confirmed"): EscalationRun => ({
  escalationId: "esc-1",
  content: "step one",
  destination: EP.destination,
  mode,
  chargedMicros: 900,
  chargedLabel: "$0.0009",
  usageReported: true,
  exceededQuote: false,
  remainingMicros: 99_100,
});

beforeEach(() => {
  store.setState({ chatMsgs: [] });
  vi.spyOn(bridge.escalation, "endpoints").mockResolvedValue([EP]);
});
afterEach(() => vi.restoreAllMocks());

describe("escalate_plan in store.handleTool (HUP-S1.5)", () => {
  it("within budget: a price notice, no approval card, run with no confirmation id", async () => {
    vi.spyOn(bridge.escalation, "quote").mockResolvedValue(quote(true));
    const r = vi.spyOn(bridge.escalation, "run").mockResolvedValue(run("budget"));
    const sig = vi.spyOn(store, "requestSig");
    const toast = vi.spyOn(store, "toast").mockImplementation(() => {});
    const out = await store.handleTool(call, "m1", noop);
    expect(sig).not.toHaveBeenCalled();
    expect(toast.mock.calls[0][0]).toMatch(/Planner · api\.example\.com: up to \$0\.005/);
    expect(r).toHaveBeenCalledWith("q-1", 5000, null, false);
    expect(out).toContain("<<<UNTRUSTED");
  });

  it("over budget and declined: the approval card shows the price, nothing runs", async () => {
    vi.spyOn(bridge.escalation, "quote").mockResolvedValue(quote(false));
    const r = vi.spyOn(bridge.escalation, "run");
    vi.spyOn(bridge.escalation, "confirmPrepare").mockResolvedValue({ confirmId: "c-1", quoteId: "q-1", costMicros: 5000, expiresMs: 0 });
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    const out = await store.handleTool(call, "m1", noop);
    expect(sig).toHaveBeenCalledTimes(1);
    const spec = sig.mock.calls[0][0];
    expect(spec.chainless).toBe(true);
    expect(spec.rows.find((x) => x.k === "Price")?.v).toContain("up to $0.005");
    expect(spec.card?.kind).toBe("fields");
    expect(r).not.toHaveBeenCalled();
    expect(out).toMatch(/declined.*nothing was sent/);
  });

  it("hic required: one card carrying the HIC reason (no extra pre-card), then run tainted + confirmed", async () => {
    vi.spyOn(bridge.escalation, "quote").mockResolvedValue(quote(true));
    const r = vi.spyOn(bridge.escalation, "run").mockResolvedValue(run("confirmed"));
    const prep = vi.spyOn(bridge.escalation, "confirmPrepare").mockResolvedValue({ confirmId: "c-1", quoteId: "q-1", costMicros: 5000, expiresMs: 0 });
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("approved");
    await store.handleTool(call, "m1", noop, { hic: "required", hicReason: "the session read untrusted content" });
    expect(sig).toHaveBeenCalledTimes(1);
    expect(sig.mock.calls[0][0].hic?.reason).toBe("the session read untrusted content");
    expect(prep).toHaveBeenCalledWith("q-1", 5000);
    expect(r).toHaveBeenCalledWith("q-1", 5000, "c-1", true);
  });

  it("the chip says declined only when the member declined, not when the answer mentions it", async () => {
    vi.spyOn(bridge.escalation, "quote").mockResolvedValue(quote(true));
    vi.spyOn(bridge.escalation, "run").mockResolvedValue({ ...run("budget"), content: "The provider declined to cache this; plan B follows." });
    vi.spyOn(store, "toast").mockImplementation(() => {});
    store.setState({ chatMsgs: [{ id: "m1", role: "assistant", text: "", chips: [] } as never] });
    await store.handleTool(call, "m1", noop);
    const chips = (store.state.chatMsgs.find((m) => m.id === "m1") as unknown as { chips: { status: string }[] }).chips;
    expect(chips.map((c) => c.status)).toEqual(["ok"]);
  });

  it("a member decline still marks the chip declined", async () => {
    vi.spyOn(bridge.escalation, "quote").mockResolvedValue(quote(false));
    vi.spyOn(bridge.escalation, "confirmPrepare").mockResolvedValue({ confirmId: "c-1", quoteId: "q-1", costMicros: 5000, expiresMs: 0 });
    vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    store.setState({ chatMsgs: [{ id: "m1", role: "assistant", text: "", chips: [] } as never] });
    await store.handleTool(call, "m1", noop);
    const chips = (store.state.chatMsgs.find((m) => m.id === "m1") as unknown as { chips: { status: string }[] }).chips;
    expect(chips.map((c) => c.status)).toEqual(["declined"]);
  });

  it("no endpoint: honest text, no quote, no card", async () => {
    vi.spyOn(bridge.escalation, "endpoints").mockResolvedValue([]);
    const q = vi.spyOn(bridge.escalation, "quote");
    const sig = vi.spyOn(store, "requestSig");
    const out = await store.handleTool(call, "m1", noop);
    expect(out).toMatch(/No escalation endpoint is set up/);
    expect(q).not.toHaveBeenCalled();
    expect(sig).not.toHaveBeenCalled();
  });
});
