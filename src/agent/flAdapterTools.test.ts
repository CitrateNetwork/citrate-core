// 0.5.4 Hermes x FL WP-H9: the adapter tools through the store's real handleTool gate, with the
// bridge spied at its boundary. fl_adapter_gate_read is a read; fl_adapter_load_propose always stops
// at the member's card (HIC-1) and core re-checks the gate; fl_adapter_unload lowers authority and
// asks nothing. Written red-first.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { store } from "../shell/store";
import { bridge } from "../bridge";
import { AGENT_TOOLS, AUTHORITY_LOWERING_AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall } from "./harness";
import { annotationFor } from "./toolAnnotations";
import { latestGateFor } from "./flAdapterTools";
import { ADAPTER_SHA, ROUND_ID, flOverview, gateRecord } from "../fl/fixtures/gate";

const call = (name: string, args: Record<string, unknown> = {}): ToolCall => ({ id: "c1", name, arguments: JSON.stringify(args) });
const noop = () => {};

beforeEach(() => {
  store.setState({ chatMsgs: [], queue: [], cerPhase: "review", walletReview: null });
});
afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ queue: [], walletReview: null });
});

describe("adapter tool schemas and annotations", () => {
  it("offers the three tools with reviewed annotations and gate lists", () => {
    const names = AGENT_TOOLS.map((t) => t.function.name);
    for (const n of ["fl_adapter_gate_read", "fl_adapter_load_propose", "fl_adapter_unload"]) expect(names).toContain(n);
    expect(annotationFor("fl_adapter_gate_read")).toEqual({ effect: "none", trust: "untrusted" });
    expect(annotationFor("fl_adapter_load_propose")).toEqual({ effect: "write", trust: "trusted" });
    expect(annotationFor("fl_adapter_unload")).toEqual({ effect: "write", trust: "trusted" });
    expect(READ_ONLY_AGENT_TOOLS.has("fl_adapter_gate_read")).toBe(true);
    expect(READ_ONLY_AGENT_TOOLS.has("fl_adapter_load_propose")).toBe(false);
    expect(AUTHORITY_LOWERING_AGENT_TOOLS.has("fl_adapter_unload")).toBe(true);
    const load = AGENT_TOOLS.find((t) => t.function.name === "fl_adapter_load_propose")!;
    expect(load.function.parameters).toMatchObject({ required: ["sha256"] });
  });

  it("no tool is both read-only and authority-lowering", () => {
    for (const n of AUTHORITY_LOWERING_AGENT_TOOLS) expect(READ_ONLY_AGENT_TOOLS.has(n)).toBe(false);
  });
});

describe("fl_adapter_gate_read", () => {
  it("returns core's verdict, the six checks and each metric, and asks nothing", async () => {
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview({ gates: [gateRecord()] }));
    const sig = vi.spyOn(store, "requestSig");
    const out = await store.handleTool(call("fl_adapter_gate_read", { sha256: ADAPTER_SHA.toUpperCase() }), "m1", noop);
    expect(sig).not.toHaveBeenCalled();
    expect(out).toContain("Verdict: ACCEPT");
    expect(out).toContain("File SHA-256 matches: passed");
    expect(out).toContain("At least three devices took part: passed");
    expect(out).toContain("correct_tool_rate: 0.910 to 0.930 (+0.020)");
    expect(out).toContain("did not read the round ledger");
    expect(out).toContain("serve no adapter");
  });

  it("says the round checks were not applicable when no round result was given", async () => {
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview({ gates: [gateRecord({ round: null })] }));
    const out = await store.handleTool(call("fl_adapter_gate_read", { sha256: ADAPTER_SHA }), "m1", noop);
    expect(out).toContain("File SHA-256 matches: passed");
    expect(out).toContain("The round result says Accepted: not applicable");
  });

  it("uses the newest decision for an adapter and fences core's reasons", async () => {
    const older = gateRecord({ decidedAtMs: 1 });
    const newer = gateRecord({ decidedAtMs: 2 }, "REJECT");
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview({ gates: [newer, older] }));
    expect(latestGateFor([older, newer], ADAPTER_SHA)).toBe(newer);
    const out = await store.handleTool(call("fl_adapter_gate_read", { sha256: ADAPTER_SHA }), "m1", noop);
    expect(out).toContain("Verdict: REJECT");
    expect(out).toMatch(/<<<UNTRUSTED[\s\S]*correct_tool_rate got worse[\s\S]*UNTRUSTED>>>/);
  });

  it("finds adapters by the round they came from", async () => {
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview({ gates: [gateRecord()] }));
    const out = await store.handleTool(call("fl_adapter_gate_read", { roundId: ROUND_ID }), "m1", noop);
    expect(out).toContain(`Adapter ${ADAPTER_SHA}`);
    const none = await store.handleTool(call("fl_adapter_gate_read", { roundId: "0x" + "1".repeat(64) }), "m1", noop);
    expect(none).toMatch(/no eval gate record/);
    expect(none).toMatch(/Rounds › Adapters/);
  });

  it("lists every adapter's newest decision, and the remembered adapter and a failed put-back", async () => {
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(
      flOverview({
        gates: [gateRecord()],
        activeAdapter: "/data/adapters/" + ADAPTER_SHA + ".gguf",
        rememberedAdapter: { sha256: ADAPTER_SHA, baseModel: "gemma-4-E4B-it-Q4_0.gguf" },
        restoreError: "The adapter you loaded earlier was not put back: the file changed",
      }),
    );
    const out = await store.handleTool(call("fl_adapter_gate_read"), "m1", noop);
    expect(out).toContain(`- ${ADAPTER_SHA}: ACCEPT on gemma-4-E4B-it-Q4_0.gguf`);
    expect(out).toContain("Remembered adapter");
    expect(out).toContain("was not put back: the file changed");
  });

  it("refuses a malformed hash and says so when core cannot be read", async () => {
    const ov = vi.spyOn(bridge.flRounds, "overview").mockRejectedValue(new Error("federated rounds need the desktop app"));
    expect(await store.handleTool(call("fl_adapter_gate_read", { sha256: "abc" }), "m1", noop)).toMatch(/64 hex/);
    expect(ov).not.toHaveBeenCalled();
    expect(await store.handleTool(call("fl_adapter_gate_read"), "m1", noop)).toMatch(/couldn't read the eval gate records: federated rounds need the desktop app/);
  });

  it("with no records says the gate needs the member's files, never a verdict", async () => {
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview());
    const out = await store.handleTool(call("fl_adapter_gate_read"), "m1", noop);
    expect(out).toMatch(/no eval gate record/);
    expect(out).not.toMatch(/ACCEPT|REJECT/);
  });
});

describe("fl_adapter_load_propose (HIC-1)", () => {
  it("shows the gate record on a card and loads nothing when the member declines", async () => {
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview({ gates: [gateRecord()] }));
    const load = vi.spyOn(bridge.flRounds, "loadAdapter");
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    const out = await store.handleTool(call("fl_adapter_load_propose", { sha256: ADAPTER_SHA }), "m1", noop);
    expect(sig).toHaveBeenCalledTimes(1);
    const spec = sig.mock.calls[0][0];
    expect(spec.chainless).toBe(true);
    expect(spec.card?.kind).toBe("fields");
    const rows = spec.rows.map((r) => r.k + "=" + r.v).join("\n");
    expect(rows).toContain(ADAPTER_SHA);
    expect(rows).toContain("ACCEPT");
    expect(rows).toContain("gemma-4-E4B-it-Q4_0.gguf");
    expect(load).not.toHaveBeenCalled();
    expect(out).toMatch(/declined/);
  });

  it("asks core to load exactly that hash after Approve", async () => {
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview({ gates: [gateRecord()] }));
    const load = vi.spyOn(bridge.flRounds, "loadAdapter").mockResolvedValue("/data/adapters/x.gguf");
    vi.spyOn(store, "requestSig").mockResolvedValue("approved");
    const out = await store.handleTool(call("fl_adapter_load_propose", { sha256: ADAPTER_SHA }), "m1", noop);
    expect(load).toHaveBeenCalledWith(ADAPTER_SHA);
    expect(out).toMatch(/Loaded adapter/);
  });

  it("reports core's refusal after Approve instead of claiming a load", async () => {
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview({ gates: [gateRecord()] }));
    vi.spyOn(bridge.flRounds, "loadAdapter").mockRejectedValue(new Error("the adapter was measured on base model a but b is selected"));
    vi.spyOn(store, "requestSig").mockResolvedValue("approved");
    const out = await store.handleTool(call("fl_adapter_load_propose", { sha256: ADAPTER_SHA }), "m1", noop);
    expect(out).toMatch(/Core refused loading the adapter: the adapter was measured on base model/);
    expect(out).not.toMatch(/^Loaded/);
  });

  it("proposes nothing for a rejected or ungated adapter", async () => {
    const sig = vi.spyOn(store, "requestSig");
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview({ gates: [gateRecord({}, "REJECT")] }));
    expect(await store.handleTool(call("fl_adapter_load_propose", { sha256: ADAPTER_SHA }), "m1", noop)).toMatch(/REJECT.*Nothing was proposed/s);
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview());
    expect(await store.handleTool(call("fl_adapter_load_propose", { sha256: ADAPTER_SHA }), "m1", noop)).toMatch(/no eval gate record.*Nothing was proposed/s);
    expect(sig).not.toHaveBeenCalled();
  });

  it("a tainted session's HIC reason rides the one card", async () => {
    vi.spyOn(bridge.flRounds, "overview").mockResolvedValue(flOverview({ gates: [gateRecord()] }));
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    await store.handleTool(call("fl_adapter_load_propose", { sha256: ADAPTER_SHA }), "m1", noop, { hic: "required", hicReason: "this session read untrusted content" });
    expect(sig).toHaveBeenCalledTimes(1);
    expect(sig.mock.calls[0][0].hic?.reason).toBe("this session read untrusted content");
  });
});

describe("fl_adapter_unload (lowers authority)", () => {
  it("unloads with no card and says the adapter is not put back", async () => {
    const sig = vi.spyOn(store, "requestSig");
    const unload = vi.spyOn(bridge.flRounds, "unloadAdapter").mockResolvedValue();
    const out = await store.handleTool(call("fl_adapter_unload"), "m1", noop);
    expect(sig).not.toHaveBeenCalled();
    expect(unload).toHaveBeenCalledTimes(1);
    expect(out).toMatch(/base model serves alone/);
  });

  it("says so when core could not unload", async () => {
    vi.spyOn(bridge.flRounds, "unloadAdapter").mockRejectedValue(new Error("internal: managed state unavailable"));
    expect(await store.handleTool(call("fl_adapter_unload"), "m1", noop)).toMatch(/couldn't unload the adapter: internal/);
  });

  it("in a tainted session it still waits for the member's card (fail closed)", async () => {
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    const unload = vi.spyOn(bridge.flRounds, "unloadAdapter").mockResolvedValue();
    await store.handleTool(call("fl_adapter_unload"), "m1", noop, { hic: "required", hicReason: "this session read untrusted content" });
    expect(sig).toHaveBeenCalledTimes(1);
    expect(unload).not.toHaveBeenCalled();
  });
});
