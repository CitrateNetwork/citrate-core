// HUP-S6.4 — the D-4 deploy gate verdict model the card renders. Pure.
import { describe, it, expect } from "vitest";
import { gateCardModel, GATE_ITEM_IDS, type DeployGateRecord, type DeployGateItem } from "./deployGate";

const H = "0xaa5f2337da9c808fb2ad5e1f956315f065d21e8369c061776c3bb6933ce0a1c6";

function item(id: DeployGateItem["id"], pass = true, reason = "ok", extra: Partial<DeployGateItem["evidence"]> = {}): DeployGateItem {
  const label = { forge_tests: "Forge tests", slither: "Slither", aderyn: "Aderyn", medusa: "Medusa campaign", fork_dry_run: "Fork dry run" }[id];
  return {
    id,
    label,
    pass,
    reason,
    evidence: { counts: { high: 0 }, outputSha256: "ab".repeat(32), durationMs: 1234, toolVersion: "1.0", ...extra },
  };
}

function record(over: Partial<DeployGateRecord> = {}): DeployGateRecord {
  return {
    initcodeHash: H,
    bindingHash: "0x" + "cd".repeat(32),
    compiler: { solcVersion: "0.8.28", optimizer: true, optimizerRuns: 200, evmVersion: "cancun", viaIr: false },
    verdict: "READY",
    items: GATE_ITEM_IDS.map((id) => item(id)),
    evaluatedAtMs: 1_700_000_000_000,
    ...over,
  };
}

describe("Feature: the deploy gate verdict card model", () => {
  it("Given a READY record with every item passing, then the card says READY and shows the hash", () => {
    const m = gateCardModel(record(), H);
    expect(m.verdict).toBe("READY");
    expect(m.ready).toBe(true);
    expect(m.initcodeHash).toBe(H);
    expect(m.reasons).toEqual([]);
    expect(m.items).toHaveLength(5);
    expect(m.compiler).toBe("solc 0.8.28 · optimizer on, 200 runs · evm cancun");
  });

  it("Given no record, then the card says NOT READY and that no gate ran for this bytecode", () => {
    const m = gateCardModel(null, H);
    expect(m.verdict).toBe("NOT READY");
    expect(m.ready).toBe(false);
    expect(m.reasons.join(" ")).toMatch(/no deploy gate has run for this exact bytecode/i);
    expect(m.initcodeHash).toBe(H);
  });

  it("Given a NOT READY record, then each failing item is a reason naming the tool", () => {
    const items = GATE_ITEM_IDS.map((id) =>
      id === "aderyn" ? item(id, false, "aderyn is not installed (a missing tool is a fail, never a pass)") : id === "slither" ? item(id, false, "1 High finding(s) (JSON)") : item(id),
    );
    const m = gateCardModel(record({ verdict: "NOT_READY", items }), H);
    expect(m.verdict).toBe("NOT READY");
    expect(m.reasons).toEqual(["Slither: 1 High finding(s) (JSON)", "Aderyn: aderyn is not installed (a missing tool is a fail, never a pass)"]);
  });

  it("Given a record for different bytecode, then the card is NOT READY (fail closed)", () => {
    const m = gateCardModel(record({ initcodeHash: "0x" + "11".repeat(32) }), H);
    expect(m.ready).toBe(false);
    expect(m.reasons.join(" ")).toMatch(/different bytecode/);
  });

  it("Given a READY label but a failing or missing item, then the card does not show READY", () => {
    const failing = gateCardModel(record({ items: GATE_ITEM_IDS.map((id) => item(id, id !== "medusa", "stopped early")) }), H);
    expect(failing.ready).toBe(false);
    expect(failing.reasons).toContain("Medusa campaign: stopped early");
    const missing = gateCardModel(record({ items: GATE_ITEM_IDS.filter((id) => id !== "fork_dry_run").map((id) => item(id)) }), H);
    expect(missing.ready).toBe(false);
    expect(missing.reasons.join(" ")).toMatch(/Fork dry run: no result/);
  });

  it("Given evidence, then each item shows its counts, run time and a short output digest", () => {
    const m = gateCardModel(record({ items: GATE_ITEM_IDS.map((id) => item(id, true, "ok", { counts: { passed: 2, failed: 0 }, durationMs: 61_500 })) }), H);
    expect(m.items[0].evidence).toBe("passed 2 · failed 0 · 61.5 s · sha256 abababababab…");
    const none = gateCardModel(record({ verdict: "NOT_READY", items: GATE_ITEM_IDS.map((id) => item(id, false, "not installed", { counts: {}, outputSha256: null, durationMs: null })) }), H);
    expect(none.items[0].evidence).toBe("no output");
  });
});
