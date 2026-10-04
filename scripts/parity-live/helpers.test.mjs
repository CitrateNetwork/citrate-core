// @vitest-environment node
// HUP-S1.9 (live parity): the HTTP tooling the live run stands on. The scripted model must serve
// the fixture's entries over real HTTP exactly as the live run assumes; the session API must make
// the same calls (method, path, bearer, body) as src-tauri/src/hermes.rs; startSidecar must refuse
// a missing binary and a binary that never answers /health.
import { describe, it, expect, afterEach } from "vitest";
import { createServer, request } from "node:http";
import { mkdtempSync, writeFileSync, chmodSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ScriptedModel } from "./scripted-model.mjs";
import { HttpSessionApi, controlCall, freeLoopbackPort, startSidecar } from "./sidecar-http.mjs";

function post(url, body, headers = {}) {
  return new Promise((resolve, reject) => {
    const u = new URL(url);
    const req = request({ host: u.hostname, port: u.port, path: u.pathname, method: "POST", headers: { "content-type": "application/json", ...headers } }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, body: Buffer.concat(chunks).toString("utf8") }));
    });
    req.on("error", reject);
    req.end(body);
  });
}

describe("ScriptedModel", () => {
  const model = new ScriptedModel({ errorStatus: 502 });
  afterEach(async () => model.stop());

  it("serves message, raw and error entries in order, records each request, then reports exhaustion", async () => {
    const base = await model.start();
    model.script([{ message: { role: "assistant", content: "hi" } }, { raw: "this is not json" }, { error: "gateway unreachable" }]);
    const a = await post(`${base}/chat/completions`, JSON.stringify({ messages: [{ role: "user", content: "1" }] }), { authorization: "Bearer k" });
    expect(a.status).toBe(200);
    expect(JSON.parse(a.body)).toEqual({ choices: [{ index: 0, message: { role: "assistant", content: "hi" } }] });
    const b = await post(`${base}/chat/completions`, "{}");
    expect(b).toEqual({ status: 200, body: "this is not json" });
    const c = await post(`${base}/chat/completions`, "{}");
    expect(c.status).toBe(502);
    const d = await post(`${base}/chat/completions`, "{}");
    expect(d.status).toBe(500);
    expect(model.requests.length).toBe(4);
    expect(model.requests[0].authorization).toBe("Bearer k");
    expect(model.requests[0].body.messages[0].content).toBe("1");
  });

  it("repeats the last entry when asked, and a new script resets the log", async () => {
    const base = await model.start();
    model.script([{ message: { role: "assistant", content: "again" } }], true);
    for (let i = 0; i < 3; i++) expect((await post(`${base}/chat/completions`, "{}")).status).toBe(200);
    expect(model.requests.length).toBe(3);
    model.script([]);
    expect(model.requests.length).toBe(0);
  });

  it("answers 404 off the completions path", async () => {
    const base = await model.start();
    model.script([{ message: { role: "assistant", content: "x" } }]);
    expect((await post(`${base}/embeddings`, "{}")).status).toBe(404);
    expect(model.requests.length).toBe(0);
  });
});

describe("HttpSessionApi (the hermes.rs session calls)", () => {
  let server;
  afterEach(async () => {
    if (server) await new Promise((r) => server.close(() => r(undefined)));
    server = undefined;
  });

  it("makes the same calls core makes, with the bearer, and records event pages without duplicates", async () => {
    const seen = [];
    server = createServer((req, res) => {
      const chunks = [];
      req.on("data", (c) => chunks.push(c));
      req.on("end", () => {
        seen.push({ method: req.method, url: req.url, auth: req.headers.authorization, body: Buffer.concat(chunks).toString("utf8") });
        res.writeHead(200, { "content-type": "application/json" });
        if (req.url === "/sessions") res.end(JSON.stringify({ id: "s1-ab" }));
        else if (req.url?.startsWith("/sessions/s1-ab/events")) res.end(JSON.stringify({ events: [{ seq: 1, event: { type: "step_start", step: 1 } }], lastSeq: 1, busy: false }));
        else res.end("{}");
      });
    });
    await new Promise((r) => server.listen(0, "127.0.0.1", () => r(undefined)));
    const base = `http://127.0.0.1:${server.address().port}`;
    const api = new HttpSessionApi(base, "tok", (p, t) => JSON.stringify({ systemPrompt: p, tools: JSON.parse(t) }));
    expect(await api.open("sys", "[]")).toBe("s1-ab");
    await api.send("s1-ab", "hello");
    await api.events("s1-ab", 0, 50);
    await api.events("s1-ab", 0, 50);
    await api.toolResult("s1-ab", "c1", "denied", "the member declined");
    await api.stop("s1-ab");
    await api.close("s1-ab");
    expect(seen.map((s) => `${s.method} ${s.url}`)).toEqual([
      "POST /sessions",
      "POST /sessions/s1-ab/messages",
      "GET /sessions/s1-ab/events?after=0&wait_ms=50",
      "GET /sessions/s1-ab/events?after=0&wait_ms=50",
      "POST /sessions/s1-ab/tool_results",
      "POST /sessions/s1-ab/stop",
      "DELETE /sessions/s1-ab",
    ]);
    expect(seen.every((s) => s.auth === "Bearer tok")).toBe(true);
    expect(JSON.parse(seen[1].body)).toEqual({ text: "hello" });
    expect(JSON.parse(seen[4].body)).toEqual({ callId: "c1", status: "denied", content: "the member declined" });
    expect(api.events_seen.length).toBe(1);
    expect(api.opened).toEqual(["s1-ab"]);
  });

  it("turns a non-2xx answer into an error that names the status", async () => {
    server = createServer((_req, res) => {
      res.writeHead(409, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: "busy" }));
    });
    await new Promise((r) => server.listen(0, "127.0.0.1", () => r(undefined)));
    const api = new HttpSessionApi(`http://127.0.0.1:${server.address().port}`, "tok", () => "{}");
    await expect(api.send("s1-ab", "x")).rejects.toThrow(/409/);
  });
});

describe("startSidecar", () => {
  it("refuses a missing binary", async () => {
    await expect(startSidecar(join(tmpdir(), "no-such-hermes-binary"))).rejects.toThrow(/no sidecar binary/);
  });

  it("reports a binary that exits before answering /health", async () => {
    const dir = mkdtempSync(join(tmpdir(), "hermes-live-parity-test-"));
    const bin = join(dir, "fake-hermes");
    writeFileSync(bin, "#!/bin/sh\necho boom >&2\nexit 3\n");
    chmodSync(bin, 0o755);
    try {
      await expect(startSidecar(bin, { readyTimeoutMs: 5_000 })).rejects.toThrow(/exited early/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("finds a free loopback port and controlCall reaches it", async () => {
    const port = await freeLoopbackPort();
    expect(port).toBeGreaterThan(0);
    const s = createServer((_req, res) => res.end("ok"));
    await new Promise((r) => s.listen(port, "127.0.0.1", () => r(undefined)));
    try {
      expect(await controlCall(`http://127.0.0.1:${port}`, "GET", "/health", null)).toEqual({ status: 200, body: "ok" });
    } finally {
      await new Promise((r) => s.close(() => r(undefined)));
    }
  });
});
