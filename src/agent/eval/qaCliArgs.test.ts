// HUP-S3.5 — argument parsing + loopback guard for scripts/eval-qa.mjs.
import { describe, it, expect } from "vitest";
import { isLoopbackUrl, parseQaCliArgs, qaDatasetFiles, qaResultFileName } from "./qaCliArgs";

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
  it("HUP-S7.7: --dataset picks a QA set by version name and refuses anything that is not one", () => {
    const base = ["--base-url", "http://127.0.0.1/v1", "--model", "m"];
    expect(parseQaCliArgs(base).dataset).toBeUndefined();
    expect(parseQaCliArgs([...base, "--dataset", "qa-literacy-v1"]).dataset).toBe("qa-literacy-v1");
    expect(parseQaCliArgs([...base, "--dataset", "qa-v1"]).dataset).toBe("qa-v1");
    for (const bad of ["../qa-v1", "qa-v1.json", "src/agent/eval/qa-v1", "QA-V1", "literacy"]) {
      expect(() => parseQaCliArgs([...base, "--dataset", bad]), bad).toThrow(/--dataset/);
    }
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
  it("HUP-S7.7: names a non-default set in the file so two sets run on one day do not collide", () => {
    expect(qaResultFileName("2026-10-01T08:00:00.000Z", "qwen", "qa-v1")).toBe("2026-10-01-qa-qwen.json");
    expect(qaResultFileName("2026-10-01T08:00:00.000Z", "qwen", "qa-literacy-v1")).toBe("2026-10-01-qa-literacy-v1-qwen.json");
  });
});

describe("qaDatasetFiles", () => {
  it("maps a set name to its dataset and anchor index under src/agent/eval, defaulting to qa-v1", () => {
    expect(qaDatasetFiles()).toEqual({ dataset: "src/agent/eval/qa-v1.json", index: "src/agent/eval/qa-v1.anchors.json" });
    expect(qaDatasetFiles("qa-literacy-v1")).toEqual({
      dataset: "src/agent/eval/qa-literacy-v1.json",
      index: "src/agent/eval/qa-literacy-v1.anchors.json",
    });
    expect(() => qaDatasetFiles("../x")).toThrow(/dataset/);
  });
});

describe("HUP-S9.4 --adapter-sha256 (eval a LoRA candidate for the gate)", () => {
  const base = ["--base-url", "http://127.0.0.1:18080/v1", "--model", "m"];
  it("accepts a sha256 hex and lowercases it", () => {
    expect(parseQaCliArgs([...base, "--adapter-sha256", "CD".repeat(32)]).adapterSha256).toBe("cd".repeat(32));
  });
  it("refuses anything that is not a sha256 hex", () => {
    expect(() => parseQaCliArgs([...base, "--adapter-sha256", "nope"])).toThrow(/sha256/);
  });
  it("names the candidate's result file so it never overwrites the base run", () => {
    expect(qaResultFileName("2026-10-01T00:00:00Z", "m", "qa-v1", "cd".repeat(32))).toBe("2026-10-01-qa-m-lora-cdcdcdcdcdcd.json");
    expect(qaResultFileName("2026-10-01T00:00:00Z", "m")).toBe("2026-10-01-qa-m.json");
  });
});

describe("retrieval flags (HUP-S3.1, g2-knowledge)", () => {
  const base = ["--base-url", "http://127.0.0.1/v1", "--model", "m"];
  const passages = ["--retrieval-mode", "passages"];
  it("defaults to the app's own path: the model calls memory_search on the tenant it picks, k as in the app", () => {
    expect(parseQaCliArgs([...base, "--memory-socket", "/tmp/memdag.sock"]).retrieval).toEqual({
      socket: "/tmp/memdag.sock",
      mode: "tool",
      tenants: ["citrate-docs", "methodology", "refs", "skills"],
      k: 5,
    });
    expect(parseQaCliArgs(base).retrieval).toBeUndefined();
  });
  it("keeps retrieve-then-answer as an explicit passages mode with tenants and k", () => {
    expect(parseQaCliArgs([...base, "--memory-socket", "/s", ...passages]).retrieval).toEqual({
      socket: "/s",
      mode: "passages",
      tenants: ["citrate-docs", "methodology"],
      k: 5,
    });
    expect(
      parseQaCliArgs([...base, "--memory-socket", "/s", ...passages, "--retrieve-tenants", "citrate-docs,refs", "--retrieve-k", "8", "--corpus-digest", "a".repeat(64)])
        .retrieval,
    ).toEqual({ socket: "/s", mode: "passages", tenants: ["citrate-docs", "refs"], k: 8, corpusDigest: "a".repeat(64) });
  });
  it("refuses tenant and k flags in tool mode: there the model and the app choose them", () => {
    expect(() => parseQaCliArgs([...base, "--memory-socket", "/s", "--retrieve-tenants", "refs"])).toThrow(/passages/);
    expect(() => parseQaCliArgs([...base, "--memory-socket", "/s", "--retrieve-k", "3"])).toThrow(/passages/);
    expect(() => parseQaCliArgs([...base, "--memory-socket", "/s", "--retrieval-mode", "magic"])).toThrow(/retrieval-mode/);
    expect(() => parseQaCliArgs([...base, "--retrieval-mode", "tool"])).toThrow(/--memory-socket/);
  });
  it("refuses runtime tenants, a bad k, retrieval flags without a socket, and a malformed digest", () => {
    expect(() => parseQaCliArgs([...base, "--memory-socket", "/s", ...passages, "--retrieve-tenants", "personal"])).toThrow(/knowledge tenant/);
    expect(() => parseQaCliArgs([...base, "--memory-socket", "/s", ...passages, "--retrieve-k", "0"])).toThrow(/retrieve-k/);
    expect(() => parseQaCliArgs([...base, "--memory-socket", "/s", ...passages, "--retrieve-k", "99"])).toThrow(/retrieve-k/);
    expect(() => parseQaCliArgs([...base, "--retrieve-k", "5"])).toThrow(/--memory-socket/);
    expect(() => parseQaCliArgs([...base, "--memory-socket", "/s", "--corpus-digest", "xyz"])).toThrow(/corpus-digest/);
  });
  it("takes the corpus directory whose nodes citations may resolve to", () => {
    expect(parseQaCliArgs([...base, "--memory-socket", "/s", "--corpus-dir", "/c"]).retrieval).toEqual({
      socket: "/s",
      mode: "tool",
      tenants: ["citrate-docs", "methodology", "refs", "skills"],
      k: 5,
      corpusDir: "/c",
    });
    expect(() => parseQaCliArgs([...base, "--corpus-dir", "/c"])).toThrow(/--memory-socket/);
  });
  it("names a retrieval run apart from the closed-book run, and a tool run apart from a passages run", () => {
    expect(qaResultFileName("2026-10-01T08:00:00.000Z", "gemma", "qa-v1", undefined, "passages")).toBe("2026-10-01-qa-rag-gemma.json");
    expect(qaResultFileName("2026-10-01T08:00:00.000Z", "gemma", "qa-literacy-v1", undefined, "passages")).toBe("2026-10-01-qa-literacy-v1-rag-gemma.json");
    expect(qaResultFileName("2026-10-01T08:00:00.000Z", "gemma", "qa-v1", undefined, "tool")).toBe("2026-10-01-qa-tool-gemma.json");
    expect(qaResultFileName("2026-10-01T08:00:00.000Z", "gemma")).toBe("2026-10-01-qa-gemma.json");
  });
});
