// US-9.2 AC2 — the belnap_codec agent tool through the real store entry point. Read-only and
// local: no approval gate, no ceremony, no bridge call.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { store } from "./store";
import type { ToolCall } from "../agent/harness";
import { READ_ONLY_AGENT_TOOLS } from "../agent/harness";
import { annotationFor } from "../agent/toolAnnotations";

const call = (args: Record<string, unknown>): ToolCall => ({ id: "c1", name: "belnap_codec", arguments: JSON.stringify(args) });
const noop = () => {};
// The output chain 40204 returned for the worked example in the citrate-belnap-aggregate skill.
const OUT = "0x000000000000d0000000000000000000000000000000200" + "0ffffffffffff700001030001";

beforeEach(() => store.setState({ chatMsgs: [] }));
afterEach(() => vi.restoreAllMocks());

describe("belnap_codec", () => {
  it("is a reviewed read-only tool with trusted output", () => {
    expect(READ_ONLY_AGENT_TOOLS.has("belnap_codec")).toBe(true);
    expect(annotationFor("belnap_codec")).toEqual({ effect: "none", trust: "trusted" });
  });

  it("decodes the worked example output into values and states without asking the member", async () => {
    const sig = vi.spyOn(store, "requestSig");
    const out = await store.handleTool(call({ mode: "decode", outputHex: OUT, dim: 4 }), "m1", noop);
    expect(sig).not.toHaveBeenCalled();
    const dec = JSON.parse(out);
    expect(dec.values).toEqual([0.8125, 0, 0.125, -0.5625]);
    expect(dec.states.map((s: { name: string }) => s.name)).toEqual(["True", "Both", "Neither", "True"]);
  });

  it("encodes participants into input bytes of the documented length", async () => {
    const out = await store.handleTool(
      call({
        mode: "encode",
        participants: [
          { embedding: [1, -0.5], confidence: [1, 1], weight: 0.5 },
          { embedding: [0.25, 0.5], confidence: [0.9, 0.9], weight: 0.5 },
        ],
        thresholdPos: 0.8,
        thresholdNeg: 0.3,
      }),
      "m1",
      noop,
    );
    const enc = JSON.parse(out);
    expect(enc.bytes).toBe(24 + 16 * 2 * 2 + 8 * 2);
    expect(enc.inputHex.startsWith("0x0000000200000002")).toBe(true);
  });

  it("a malformed call is explained, never guessed", async () => {
    const out = await store.handleTool(call({ mode: "decode", outputHex: "0x00", dim: 4 }), "m1", noop);
    expect(out).toMatch(/36 bytes/);
    expect(out).toMatch(/Nothing was computed/);
  });
});
