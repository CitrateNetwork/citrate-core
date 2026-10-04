// HUP-S1.7 — the eval CLI's argument parser. The loopback guard is the point: a run sends
// the full system prompt + tool schemas (and, for injection cases, a canary secret) to the
// endpoint, so a non-loopback URL is refused unless --allow-remote is explicit.
import { describe, it, expect } from "vitest";
import { isLoopbackUrl, parseEvalCliArgs, resultFileName, loraRequestField } from "./cliArgs";

describe("isLoopbackUrl", () => {
  it("accepts 127.0.0.0/8, localhost and ::1 over http(s)", () => {
    for (const u of [
      "http://127.0.0.1:18080/v1",
      "http://127.8.9.10/v1",
      "http://localhost:8080/v1",
      "https://localhost/v1",
      "http://[::1]:18080/v1",
    ]) {
      expect(isLoopbackUrl(u)).toBe(true);
    }
  });
  it("rejects remote hosts, look-alikes, 0.0.0.0, other schemes and garbage", () => {
    for (const u of [
      "https://api.example.com/v1",
      "http://127.0.0.1.nip.io/v1",
      "http://localhost.evil.test/v1",
      "http://0.0.0.0:18080/v1",
      "http://10.0.0.5:18080/v1",
      "http://127.0.0.1@evil.test/v1",
      "ftp://127.0.0.1/v1",
      "file:///etc/passwd",
      "not a url",
    ]) {
      expect(isLoopbackUrl(u)).toBe(false);
    }
  });
});

describe("parseEvalCliArgs", () => {
  it("parses the documented flags", () => {
    const a = parseEvalCliArgs([
      "--base-url",
      "http://127.0.0.1:18080/v1",
      "--model",
      "qwen3.6-27b-q4",
      "--api-key-env",
      "EVAL_KEY",
      "--tier",
      "T1",
    ]);
    expect(a).toEqual({
      baseUrl: "http://127.0.0.1:18080/v1",
      model: "qwen3.6-27b-q4",
      apiKeyEnv: "EVAL_KEY",
      allowRemote: false,
      tier: "T1",
      outDir: "eval/results",
    });
  });
  it("strips a trailing slash from the base url", () => {
    expect(parseEvalCliArgs(["--base-url", "http://localhost:1/v1/", "--model", "m"]).baseUrl).toBe("http://localhost:1/v1");
  });
  it("requires --base-url and --model", () => {
    expect(() => parseEvalCliArgs(["--model", "m"])).toThrow(/--base-url/);
    expect(() => parseEvalCliArgs(["--base-url", "http://127.0.0.1/v1"])).toThrow(/--model/);
  });
  it("REFUSES a non-loopback url without --allow-remote", () => {
    expect(() => parseEvalCliArgs(["--base-url", "https://api.example.com/v1", "--model", "m"])).toThrow(/loopback/);
  });
  it("accepts a non-loopback url only with --allow-remote", () => {
    const a = parseEvalCliArgs(["--base-url", "https://api.example.com/v1", "--model", "m", "--allow-remote"]);
    expect(a.allowRemote).toBe(true);
  });
  it("still rejects a non-http(s) url even with --allow-remote", () => {
    expect(() => parseEvalCliArgs(["--base-url", "file:///x", "--model", "m", "--allow-remote"])).toThrow(/http/);
  });
  it("rejects unknown flags, a flag missing its value, a bad tier and a bad env var name", () => {
    expect(() => parseEvalCliArgs(["--base-url", "http://127.0.0.1/v1", "--model", "m", "--yolo"])).toThrow(/unknown/i);
    expect(() => parseEvalCliArgs(["--base-url", "http://127.0.0.1/v1", "--model"])).toThrow(/--model/);
    expect(() => parseEvalCliArgs(["--base-url", "http://127.0.0.1/v1", "--model", "m", "--tier", "T9"])).toThrow(/tier/);
    expect(() =>
      parseEvalCliArgs(["--base-url", "http://127.0.0.1/v1", "--model", "m", "--api-key-env", "sk-live-abc"]),
    ).toThrow(/env/i);
  });
});

describe("resultFileName", () => {
  it("is <date>-<model>.json with the model name made path-safe", () => {
    expect(resultFileName("2026-09-30T12:00:00.000Z", "qwen/Qwen3.6-27B:Q4_K_M")).toBe(
      "2026-09-30-qwen_Qwen3.6-27B_Q4_K_M.json",
    );
    expect(resultFileName("2026-09-30T12:00:00.000Z", "../../etc")).toBe("2026-09-30-______etc.json");
  });
});

describe("HUP-S9.4 --adapter-sha256 (eval a LoRA candidate for the gate)", () => {
  const base = ["--base-url", "http://127.0.0.1:18080/v1", "--model", "m"];
  it("accepts a sha256 hex and lowercases it", () => {
    expect(parseEvalCliArgs([...base, "--adapter-sha256", "AB".repeat(32)]).adapterSha256).toBe("ab".repeat(32));
  });
  it("refuses anything that is not a sha256 hex", () => {
    expect(() => parseEvalCliArgs([...base, "--adapter-sha256", "xyz"])).toThrow(/sha256/);
    expect(() => parseEvalCliArgs([...base, "--adapter-sha256", "a".repeat(63)])).toThrow(/sha256/);
  });
  it("is absent by default (a base run)", () => {
    expect(parseEvalCliArgs(base).adapterSha256).toBeUndefined();
  });
  it("names the candidate's result file so it never overwrites the base run", () => {
    expect(resultFileName("2026-10-01T00:00:00Z", "m", "ab".repeat(32))).toBe("2026-10-01-m-lora-abababababab.json");
    expect(resultFileName("2026-10-01T00:00:00Z", "m")).toBe("2026-10-01-m.json");
  });
});

describe("HUP-S9.4 --lora-scale (both arms on one server started with --lora-scaled <file>:0)", () => {
  const base = ["--base-url", "http://127.0.0.1:18080/v1", "--model", "m"];
  const sha = "cd".repeat(32);
  it("is absent by default", () => {
    expect(parseEvalCliArgs(base).loraScale).toBeUndefined();
  });
  it("a candidate arm needs the adapter's sha256", () => {
    expect(() => parseEvalCliArgs([...base, "--lora-scale", "1"])).toThrow(/--adapter-sha256/);
    expect(parseEvalCliArgs([...base, "--lora-scale", "1", "--adapter-sha256", sha]).loraScale).toBe(1);
  });
  it("a base arm (scale 0) must not be stamped with an adapter", () => {
    expect(parseEvalCliArgs([...base, "--lora-scale", "0"]).loraScale).toBe(0);
    expect(() => parseEvalCliArgs([...base, "--lora-scale", "0", "--adapter-sha256", sha])).toThrow(/base run/);
  });
  it("refuses a scale that is not a number from 0 to 1", () => {
    for (const bad of ["-1", "1.5", "abc", "NaN", "Infinity"]) {
      expect(() => parseEvalCliArgs([...base, "--lora-scale", bad, "--adapter-sha256", sha])).toThrow(/--lora-scale/);
    }
  });
  it("the request field addresses adapter id 0 at the given scale", () => {
    expect(loraRequestField(0)).toEqual({ lora: [{ id: 0, scale: 0 }] });
    expect(loraRequestField(1)).toEqual({ lora: [{ id: 0, scale: 1 }] });
    expect(loraRequestField(undefined)).toEqual({});
  });
});
