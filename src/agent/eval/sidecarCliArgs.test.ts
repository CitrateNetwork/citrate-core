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
  it("--allow-remote admits an https endpoint (CI, eval.yml) but never plain http off loopback", () => {
    const remote = (url: string) => BASE.map((v) => (v.startsWith("http") ? url : v));
    const a = parseSidecarEvalArgs([...remote("https://eval.example/v1/"), "--allow-remote"]);
    expect(a.baseUrl).toBe("https://eval.example/v1");
    expect(a.allowRemote).toBe(true);
    // The sidecar itself refuses plain http to a non-loopback host (sessions.rs validate_endpoint).
    expect(() => parseSidecarEvalArgs([...remote("http://10.0.0.5:8080/v1"), "--allow-remote"])).toThrow(/https/);
    expect(() => parseSidecarEvalArgs([...remote("ftp://eval.example/v1"), "--allow-remote"])).toThrow(/https/);
    // Loopback needs no flag and stays allowRemote=false.
    expect(parseSidecarEvalArgs(BASE).allowRemote).toBe(false);
  });
  it("--runtime-rev stamps the runtime commit the sidecar was built from (a full sha only)", () => {
    const rev = "45fceda902dcaaec29060a82ffeba705dc12b0a6";
    expect(parseSidecarEvalArgs([...BASE, "--runtime-rev", rev]).runtimeRev).toBe(rev);
    expect(parseSidecarEvalArgs(BASE).runtimeRev).toBeUndefined();
    expect(() => parseSidecarEvalArgs([...BASE, "--runtime-rev", "45fceda"])).toThrow(/--runtime-rev/);
    expect(() => parseSidecarEvalArgs([...BASE, "--runtime-rev", "main"])).toThrow(/--runtime-rev/);
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
