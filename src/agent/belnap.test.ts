// @vitest-environment node
//
// US-9.2 AC2 — the 0x0110 encoder/decoder against the bundled skill's worked example. The hex is
// read from src-tauri/skills/citrate-belnap-aggregate/SKILL.md itself, so the skill and the tool
// cannot drift apart: the input block must equal what encodeBelnapInput produces for the example
// table, and decoding the output chain 40204 returned must give the hand-calculated values/states.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { belnapCodecTool, cleanHex, decodeBelnapOutput, encodeBelnapInput, toQ16 } from "./belnap";

const SKILL = readFileSync(resolve(process.cwd(), "src-tauri/skills/citrate-belnap-aggregate/SKILL.md"), "utf8");

/** The two ```text blocks under "Worked example": input, then output. */
function workedExampleHex(): { input: string; output: string } {
  const section = SKILL.slice(SKILL.indexOf("## Worked example"));
  const blocks = [...section.matchAll(/```text\n([\s\S]*?)```/g)].map((m) => m[1]);
  if (blocks.length < 2) throw new Error("worked example blocks not found in SKILL.md");
  return { input: cleanHex(blocks[0]), output: cleanHex(blocks[1]) };
}

/** The example table: 3 participants, 4 dimensions, threshold_pos 0.8, threshold_neg 0.3. */
const EXAMPLE = {
  participants: [
    { embedding: [1.0, 1.0, 0.25, -1.0], confidence: [1.0, 1.0, 0.25, 1.0], weight: 0.5 },
    { embedding: [0.5, -1.0, -0.25, -0.5], confidence: [1.0, 1.0, 0.25, 1.0], weight: 0.5 },
    { embedding: [0.25, 0.0, 0.5, 0.75], confidence: [0.5, 0.5, 0.25, 0.5], weight: 0.25 },
  ],
  thresholdPos: 0.8,
  thresholdNeg: 0.3,
};

describe("Q16 conversion", () => {
  it("matches the skill's examples, rounds half away from zero, and clamps to i64", () => {
    expect(toQ16(1)).toBe(65536n);
    expect(toQ16(0.5)).toBe(32768n);
    expect(toQ16(-1)).toBe(-65536n);
    expect(toQ16(0.8)).toBe(52429n);
    expect(toQ16(-0.8)).toBe(-52429n);
    expect(toQ16(1e300)).toBe(2n ** 63n - 1n);
    expect(toQ16(-1e300)).toBe(-(2n ** 63n));
    expect(() => toQ16(Number.NaN)).toThrow(/finite/);
  });
});

describe("Feature: encode 0x0110 input (US-9.2 AC2)", () => {
  it("Given the worked example table, then the bytes equal the SKILL.md input block (240 bytes)", () => {
    const enc = encodeBelnapInput(EXAMPLE);
    expect(enc.bytes).toBe(240);
    expect(enc.dim).toBe(4);
    expect(enc.n).toBe(3);
    expect(enc.outputBytes).toBe(36);
    expect(enc.gasBudget).toBe(2000 + 50 * 4 * 3);
    expect(cleanHex(enc.inputHex)).toBe(workedExampleHex().input);
  });

  it("refuses mismatched dimensions, empty input and non-numbers instead of encoding garbage", () => {
    const bad = { ...EXAMPLE, participants: [EXAMPLE.participants[0], { ...EXAMPLE.participants[1], embedding: [1, 2, 3] }] };
    expect(() => encodeBelnapInput(bad)).toThrow(/same dimension/);
    expect(() => encodeBelnapInput({ ...EXAMPLE, participants: [] })).toThrow(/1 to 1024/);
    const nan = { ...EXAMPLE, participants: [{ ...EXAMPLE.participants[0], weight: Number.NaN }] };
    expect(() => encodeBelnapInput(nan)).toThrow(/weight/);
    expect(() => encodeBelnapInput({ ...EXAMPLE, thresholdPos: "0.8" as unknown as number })).toThrow(/thresholdPos/);
  });
});

describe("Feature: decode 0x0110 output (US-9.2 AC2)", () => {
  it("Given the output chain 40204 returned, then values and states match the hand calculation", () => {
    const dec = decodeBelnapOutput(workedExampleHex().output, 4);
    expect(dec.values).toEqual([0.8125, 0, 0.125, -0.5625]);
    expect(dec.rawValues).toEqual(["53248", "0", "8192", "-36864"]);
    expect(dec.states.map((s) => s.name)).toEqual(["True", "Both", "Neither", "True"]);
    expect(dec.states.map((s) => s.code)).toEqual([1, 3, 0, 1]);
  });

  it("refuses a wrong length, a non-hex string and an unknown state byte", () => {
    const out = workedExampleHex().output;
    expect(() => decodeBelnapOutput(out, 3)).toThrow(/27 bytes/);
    expect(() => decodeBelnapOutput("0xzz", 1)).toThrow(/hex/);
    expect(() => decodeBelnapOutput("00".repeat(8) + "07", 1)).toThrow(/state byte 7/);
  });
});

describe("the belnap_codec tool wrapper", () => {
  it("encodes and decodes through mode, and explains a bad call without computing anything", () => {
    const enc = JSON.parse(belnapCodecTool({ mode: "encode", ...EXAMPLE }));
    expect(cleanHex(enc.inputHex)).toBe(workedExampleHex().input);
    const dec = JSON.parse(belnapCodecTool({ mode: "decode", outputHex: workedExampleHex().output, dim: "4" }));
    expect(dec.states.map((s: { name: string }) => s.name)).toEqual(["True", "Both", "Neither", "True"]);
    expect(belnapCodecTool({ mode: "aggregate" })).toMatch(/Nothing was computed/);
    expect(belnapCodecTool({ mode: "decode" })).toMatch(/outputHex/);
  });
});
