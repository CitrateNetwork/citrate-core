// HUP-S6.2 / S6.9 — the forge panel's pure helpers. Core validates and decides; these only shape
// what the form sends and how core's answer reads.
import { describe, it, expect } from "vitest";
import { artifactFor, initialValues, medusaLine, renderParams, toolchainGateRequest, type TemplateEntry } from "./contractForge";

const ERC20: TemplateEntry = {
  id: "erc20",
  kind: "contract",
  title: "ERC-20 fixed supply (OpenZeppelin)",
  description: "",
  medusa: true,
  fields: [
    { key: "name", label: "Name", help: "", default: null, min: null, max: null, required: true },
    { key: "symbol", label: "Symbol", help: "", default: null, min: null, max: null, required: true },
    { key: "supply", label: "Supply", help: "", default: "1000000", min: 1, max: 1_000_000_000_000, required: false },
    { key: "owner", label: "Owner address", help: "", default: null, min: null, max: null, required: true },
  ],
};

describe("contract forge helpers", () => {
  it("starts each field at its default", () => {
    expect(initialValues(ERC20)).toEqual({ name: "", symbol: "", supply: "1000000", owner: "" });
  });

  it("sends trimmed values, leaves empty optional fields to the renderer, and names missing required ones", () => {
    const r = renderParams(ERC20, { name: " Lemon Drops ", symbol: "LEMON", supply: "", owner: "" });
    expect(r.params).toEqual({ name: "Lemon Drops", symbol: "LEMON" });
    expect(r.missing).toEqual(["Owner address"]);
  });

  it("explains the tier check in plain words", () => {
    const ok = medusaLine({ tier: "T0", requiredCalls: 10000, minCoveragePct: 60, runTestLimit: 10000, linesHit: 3, linesTotal: 3, coveragePct: 100, problems: [] });
    expect(ok).toBe("T0: at least 10,000 calls and 60% line coverage; 100% of src/ lines covered (3 of 3).");
    const bad = medusaLine({ tier: "T1", requiredCalls: 50000, minCoveragePct: 75, runTestLimit: 10000, linesHit: null, linesTotal: null, coveragePct: null, problems: ["the campaign wrote no coverage report"] });
    expect(bad).toMatch(/no coverage measured\. Not accepted: the campaign wrote no coverage report$/);
  });

  it("names forge's artifact for a source file and contract", () => {
    expect(artifactFor("src/Token.sol", "LemonDrops")).toBe("Token.sol/LemonDrops.json");
    expect(artifactFor("Token.sol", "X")).toBe("Token.sol/X.json");
  });
});

describe("HUP-S6.10: the forge panel's gate request", () => {
  it("always asks core to run the dry run on the Citrate-aware fork (forkInCore)", () => {
    expect(toolchainGateRequest(" s1-ab ", "/p ", " Token.sol/LemonDrops.json")).toEqual({
      sessionId: "s1-ab",
      project: "/p",
      artifact: "Token.sol/LemonDrops.json",
      forkInCore: {},
    });
  });
});
