// HUP-S3.5 — argument parsing + loopback guard for scripts/eval-qa.mjs.
import { describe, it, expect } from "vitest";
import { isLoopbackUrl, parseQaCliArgs, qaResultFileName } from "./qaCliArgs";

describe("isLoopbackUrl", () => {
  it("accepts 127.0.0.0/8, localhost and ::1 over http(s)", () => {
    for (const u of ["http://127.0.0.1:18080/v1", "http://127.9.9.9/v1", "http://localhost:8080", "http://[::1]:1/v1", "https://localhost/v1"]) {
      expect(isLoopbackUrl(u)).toBe(true);
    }
  });
  it("rejects routable hosts, look-alikes and non-http schemes", () => {
    for (const u of ["http://10.0.0.5/v1", "https://api.example.com/v1", "http://127.0.0.1.example.com/v1", "http://128.0.0.1/", "ftp://127.0.0.1/", "not a url"]) {
      expect(isLoopbackUrl(u)).toBe(false);
    }
  });
});

describe("parseQaCliArgs", () => {
  it("parses a loopback run with defaults", () => {
    expect(parseQaCliArgs(["--base-url", "http://127.0.0.1:18080/v1/", "--model", "qwen"])).toEqual({
      baseUrl: "http://127.0.0.1:18080/v1",
      model: "qwen",
      allowRemote: false,
      outDir: "eval/results",
    });
  });
  it("refuses a non-loopback endpoint unless --allow-remote is passed", () => {
    expect(() => parseQaCliArgs(["--base-url", "https://api.example.com/v1", "--model", "m"])).toThrow(/non-loopback/);
    expect(parseQaCliArgs(["--base-url", "https://api.example.com/v1", "--model", "m", "--allow-remote"]).allowRemote).toBe(true);
  });
  it("requires base-url and model, and validates tier, api-key-env and threshold", () => {
    expect(() => parseQaCliArgs(["--model", "m"])).toThrow(/--base-url is required/);
    expect(() => parseQaCliArgs(["--base-url", "http://127.0.0.1/v1"])).toThrow(/--model is required/);
    const base = ["--base-url", "http://127.0.0.1/v1", "--model", "m"];
    expect(() => parseQaCliArgs([...base, "--tier", "T9"])).toThrow(/tier/);
    expect(() => parseQaCliArgs([...base, "--api-key-env", "sk-abc123"])).toThrow(/NAME/);
    expect(() => parseQaCliArgs([...base, "--coverage-threshold", "1.5"])).toThrow(/threshold/);
    expect(parseQaCliArgs([...base, "--tier", "T1", "--api-key-env", "EVAL_API_KEY", "--coverage-threshold", "0.75"])).toMatchObject({
      tier: "T1",
      apiKeyEnv: "EVAL_API_KEY",
      coverageThreshold: 0.75,
    });
  });
  it("rejects unknown flags and flags missing their value", () => {
    expect(() => parseQaCliArgs(["--base-url", "http://127.0.0.1/v1", "--model", "m", "--judge", "gpt"])).toThrow(/unknown/);
    expect(() => parseQaCliArgs(["--base-url", "--model", "m"])).toThrow(/needs a value/);
  });
});

describe("qaResultFileName", () => {
  it("is dated, prefixed qa-, and reduces the model name to one safe path segment", () => {
    expect(qaResultFileName("2026-09-30T12:00:00.000Z", "../../etc/passwd")).toBe("2026-09-30-qa-______etc_passwd.json");
    expect(qaResultFileName("2026-09-30T12:00:00.000Z", "qwen2.5-7b")).toBe("2026-09-30-qa-qwen2.5-7b.json");
  });
});
