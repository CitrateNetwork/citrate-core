// HUP-S1.7 / S1.10 — scripts/eval-sidecar.mjs argument guards.
import { describe, expect, it } from "vitest";
import { parseSidecarEvalArgs } from "./sidecarCliArgs";

const BASE = [
  "--base-url", "http://127.0.0.1:18190/v1/",
  "--model", "gemma",
  "--sidecar-bin", "/opt/x/citrate-agent-sidecar",
  "--mcp-fixture-bin", "/opt/x/citrate-mcp-fixture-server",
  "--context-tokens", "16384",
  "--chromium", "/Applications/Chrome",
];

describe("parseSidecarEvalArgs", () => {
  it("parses a full run and derives core's reply cap", () => {
    const a = parseSidecarEvalArgs([...BASE, "--tier", "T0"]);
    expect(a.baseUrl).toBe("http://127.0.0.1:18190/v1");
    expect(a.maxTokens).toBe(2048);
    expect(a.deadlineSeconds).toBe(900);
    expect(a.tier).toBe("T0");
    expect(parseSidecarEvalArgs(BASE.map((v) => (v === "16384" ? "4096" : v))).maxTokens).toBe(1024);
  });
  it("refuses a non-loopback endpoint, relative executables and a missing context size", () => {
    expect(() => parseSidecarEvalArgs(BASE.map((v) => (v.startsWith("http") ? "https://api.example.com/v1" : v)))).toThrow(/loopback/);
    expect(() => parseSidecarEvalArgs(BASE.map((v) => (v.endsWith("citrate-agent-sidecar") ? "./sidecar" : v)))).toThrow(/absolute/);
    const noCtx = BASE.filter((v, i) => v !== "--context-tokens" && BASE[i - 1] !== "--context-tokens");
    expect(() => parseSidecarEvalArgs(noCtx)).toThrow(/--context-tokens is required/);
  });
  it("needs a browser for injection cases unless only workflows run", () => {
    const noChrome = BASE.slice(0, -2);
    expect(() => parseSidecarEvalArgs(noChrome)).toThrow(/--chromium/);
    expect(parseSidecarEvalArgs([...noChrome, "--only", "workflows"]).only).toBe("workflows");
  });
  it("refuses an API key on argv, unknown flags and bad values", () => {
    expect(() => parseSidecarEvalArgs([...BASE, "--api-key-env", "lowercase-value"])).toThrow(/NAME/);
    expect(() => parseSidecarEvalArgs([...BASE, "--fast"])).toThrow(/unknown argument/);
    expect(() => parseSidecarEvalArgs([...BASE, "--only", "all"])).toThrow(/--only/);
    expect(() => parseSidecarEvalArgs([...BASE, "--deadline-s", "5"])).toThrow(/--deadline-s/);
  });
});
