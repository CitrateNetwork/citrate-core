// HUP-S1.9 (live parity): a scripted OpenAI-compatible model endpoint for the live parity run.
//
// Test tooling, not part of the app. The live run points a real sidecar session's `llm.baseUrl`
// at this server, so every byte the sidecar sends to its model and every byte it parses back
// crosses a real HTTP connection. Each `POST .../chat/completions` takes the next scripted entry
// of parity-v1.json:
//   - `{message}` -> 200 `{"choices":[{"index":0,"message":...}]}`
//   - `{raw}`     -> 200 with that exact body (a body that is not JSON, for the wire scenarios)
//   - `{error}`   -> `errorStatus` (default 502). The sidecar and core both report only the status
//                    of a failed model call, never its body, so the live check is the status.
// Every request body is recorded (parsed JSON) so the run can compare what the model saw.
import { createServer } from "node:http";

export class ScriptedModel {
  /** @param {{ errorStatus?: number }} [opts] */
  constructor(opts = {}) {
    this.errorStatus = opts.errorStatus ?? 502;
    /** @type {import("node:http").Server | null} */
    this.server = null;
    /** @type {Array<{message?: object, raw?: string, error?: string}>} */
    this.entries = [];
    this.repeatLast = false;
    this.idx = 0;
    /** @type {Array<{path: string, authorization: string | null, body: any}>} */
    this.requests = [];
  }

  /** Replace the script; resets the request log and the cursor. */
  script(entries, repeatLast = false) {
    this.entries = entries;
    this.repeatLast = repeatLast;
    this.idx = 0;
    this.requests = [];
  }

  /** Listen on an ephemeral loopback port; resolves to the `.../v1` base URL. */
  async start() {
    if (this.server) throw new Error("scripted model already started");
    const server = createServer((req, res) => {
      void this.handle(req, res);
    });
    this.server = server;
    await new Promise((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", () => resolve(undefined));
    });
    const addr = server.address();
    const port = typeof addr === "object" && addr ? addr.port : 0;
    return `http://127.0.0.1:${port}/v1`;
  }

  async stop() {
    const s = this.server;
    this.server = null;
    if (!s) return;
    s.closeAllConnections();
    await new Promise((resolve) => s.close(() => resolve(undefined)));
  }

  /** @param {import("node:http").IncomingMessage} req @param {import("node:http").ServerResponse} res */
  async handle(req, res) {
    const chunks = [];
    for await (const c of req) chunks.push(c);
    const text = Buffer.concat(chunks).toString("utf8");
    if (req.method !== "POST" || !(req.url ?? "").endsWith("/chat/completions")) {
      res.writeHead(404, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: { message: "not found" } }));
      return;
    }
    let body;
    try {
      body = JSON.parse(text);
    } catch {
      body = { unparsed: text };
    }
    this.requests.push({ path: req.url ?? "", authorization: req.headers.authorization ?? null, body });
    const exhausted = this.entries.length === 0 || (this.idx >= this.entries.length && !this.repeatLast);
    if (exhausted) {
      res.writeHead(500, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: { message: "parity script exhausted" } }));
      return;
    }
    const entry = this.entries[Math.min(this.idx, this.entries.length - 1)];
    this.idx++;
    if (entry.error !== undefined) {
      res.writeHead(this.errorStatus, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: { message: entry.error } }));
      return;
    }
    res.writeHead(200, { "content-type": "application/json" });
    if (entry.raw !== undefined) {
      res.end(entry.raw);
      return;
    }
    res.end(JSON.stringify({ choices: [{ index: 0, message: entry.message ?? {} }] }));
  }
}
