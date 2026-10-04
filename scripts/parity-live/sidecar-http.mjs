// HUP-S1.9 (live parity): drive a real Hermes sidecar binary over its loopback control API.
//
// Test tooling, not part of the app. In the app, the webview's `bridge.agentHarness` calls the
// `hermes_session_*` commands and src-tauri/src/hermes.rs turns each into one HTTP call with the
// session bearer. This module makes the same calls with the same paths and bodies (pinned on the
// Rust side by hermes_session_tests.rs), so the live run exercises the real sidecar process: its
// session layer, its event log, its core-host `tool_results` round trip and its model client.
// The spawn mirrors `HermesManager::build_spec`: the control bind and the bearer FILE path travel
// in env, never the token itself.
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdtempSync, writeFileSync, chmodSync, rmSync, mkdirSync, cpSync, existsSync } from "node:fs";
import { request } from "node:http";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

/**
 * One HTTP call to a loopback control plane.
 * @returns {Promise<{status: number, body: string}>}
 */
export function controlCall(baseUrl, method, path, bearer, body, timeoutMs = 30_000) {
  return new Promise((resolve, reject) => {
    const u = new URL(path, baseUrl);
    /** @type {Record<string, string>} */
    const headers = {};
    if (bearer !== null) headers.authorization = `Bearer ${bearer}`;
    if (body !== undefined) {
      headers["content-type"] = "application/json";
      headers["content-length"] = String(Buffer.byteLength(body));
    }
    const req = request({ host: u.hostname, port: u.port, path: u.pathname + u.search, method, headers, timeout: timeoutMs }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode ?? 0, body: Buffer.concat(chunks).toString("utf8") }));
    });
    req.on("timeout", () => req.destroy(new Error(`control call ${method} ${u.pathname} timed out`)));
    req.on("error", reject);
    if (body !== undefined) req.write(body);
    req.end();
  });
}

function ensureOk(r, what) {
  if (r.status < 200 || r.status >= 300) throw new Error(`hermes control returned ${r.status} for ${what}: ${r.body.slice(0, 200)}`);
  return r;
}

/**
 * core's `SidecarSessionApi` over HTTP, plus `close` and a record of every event page and every
 * opened session id. `buildOpenBody(systemPrompt, toolsJson)` returns the `POST /sessions` body.
 */
export class HttpSessionApi {
  constructor(baseUrl, bearer, buildOpenBody) {
    this.baseUrl = baseUrl;
    this.bearer = bearer;
    this.buildOpenBody = buildOpenBody;
    /** @type {Array<{seq: number, event: Record<string, unknown>}>} */
    this.events_seen = [];
    /** @type {string[]} */
    this.opened = [];
  }

  async open(systemPrompt, toolsJson) {
    const r = ensureOk(await controlCall(this.baseUrl, "POST", "/sessions", this.bearer, this.buildOpenBody(systemPrompt, toolsJson)), "POST /sessions");
    const id = JSON.parse(r.body).id;
    if (typeof id !== "string" || id.length === 0) throw new Error("hermes control returned no session id");
    this.opened.push(id);
    return id;
  }

  async send(id, text) {
    ensureOk(await controlCall(this.baseUrl, "POST", `/sessions/${id}/messages`, this.bearer, JSON.stringify({ text })), "POST messages");
  }

  async events(id, after, waitMs) {
    const r = ensureOk(await controlCall(this.baseUrl, "GET", `/sessions/${id}/events?after=${after}&wait_ms=${waitMs}`, this.bearer, undefined, waitMs + 30_000), "GET events");
    const page = JSON.parse(r.body);
    for (const e of page.events) {
      if (!this.events_seen.some((s) => s.seq === e.seq)) this.events_seen.push(e);
    }
    return page;
  }

  async toolResult(id, callId, status, content) {
    ensureOk(await controlCall(this.baseUrl, "POST", `/sessions/${id}/tool_results`, this.bearer, JSON.stringify({ callId, status, content })), "POST tool_results");
  }

  async stop(id) {
    ensureOk(await controlCall(this.baseUrl, "POST", `/sessions/${id}/stop`, this.bearer, "{}"), "POST stop");
  }

  async close(id) {
    ensureOk(await controlCall(this.baseUrl, "DELETE", `/sessions/${id}`, this.bearer), "DELETE session");
  }
}

/** A free loopback TCP port (bound, then released). */
export function freeLoopbackPort() {
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.once("error", reject);
    s.listen(0, "127.0.0.1", () => {
      const addr = s.address();
      const port = typeof addr === "object" && addr ? addr.port : 0;
      s.close(() => resolve(port));
    });
  });
}

/**
 * Spawn the sidecar binary the way core does: `CITRATE_HERMES_ADDR`, `CITRATE_HERMES_TOKEN_FILE`
 * (a fresh 256-bit bearer in a 0600 file), `CITRATE_HERMES_CHECKPOINTS`, and
 * `CITRATE_HERMES_CAPSULES` (a copy of the bundled capsules) when a dir is given. Only PATH and a
 * scratch HOME come from outside, so no developer setting leaks into the run.
 * @param {string} bin
 * @param {{ capsulesDir?: string, readyTimeoutMs?: number, extraEnv?: Record<string, string> }} [opts]
 */
export async function startSidecar(bin, opts = {}) {
  if (!existsSync(bin)) throw new Error(`no sidecar binary at ${bin}`);
  const dir = mkdtempSync(join(tmpdir(), "hermes-live-parity-"));
  const bearer = randomBytes(32).toString("hex");
  const tokenFile = join(dir, "hermes.token");
  writeFileSync(tokenFile, bearer, { mode: 0o600 });
  chmodSync(tokenFile, 0o600);
  const port = await freeLoopbackPort();
  const addr = `127.0.0.1:${port}`;
  mkdirSync(join(dir, "checkpoints"), { recursive: true });
  const env = {
    PATH: process.env.PATH ?? "/usr/bin:/bin",
    HOME: dir,
    CITRATE_HERMES_ADDR: addr,
    CITRATE_HERMES_TOKEN_FILE: tokenFile,
    CITRATE_HERMES_CHECKPOINTS: join(dir, "checkpoints"),
    ...(opts.extraEnv ?? {}),
  };
  if (opts.capsulesDir && existsSync(opts.capsulesDir)) {
    const caps = join(dir, "capsules");
    cpSync(opts.capsulesDir, caps, { recursive: true });
    env.CITRATE_HERMES_CAPSULES = caps;
  }
  const child = spawn(bin, [], { env, cwd: dir, stdio: ["ignore", "ignore", "pipe"] });
  let err = "";
  child.stderr?.on("data", (c) => {
    err = (err + c.toString("utf8")).slice(-20_000);
  });
  const baseUrl = `http://${addr}`;
  const cleanup = () => rmSync(dir, { recursive: true, force: true });
  const deadline = Date.now() + (opts.readyTimeoutMs ?? 20_000);
  for (;;) {
    if (child.exitCode !== null || child.signalCode !== null) {
      cleanup();
      throw new Error(`the sidecar exited early (${child.exitCode ?? child.signalCode}): ${err.slice(-2000)}`);
    }
    try {
      const h = await controlCall(baseUrl, "GET", "/health", null, undefined, 2_000);
      if (h.status === 200) break;
    } catch {
      // not listening yet
    }
    if (Date.now() > deadline) {
      child.kill("SIGKILL");
      cleanup();
      throw new Error(`the sidecar did not answer /health in time: ${err.slice(-2000)}`);
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  const stop = async () => {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGTERM");
      await new Promise((resolve) => {
        const t = setTimeout(() => {
          child.kill("SIGKILL");
          resolve(undefined);
        }, 5_000);
        child.once("exit", () => {
          clearTimeout(t);
          resolve(undefined);
        });
      });
    }
    cleanup();
  };
  return { baseUrl, bearer, child, dir, stderr: () => err, stop };
}
