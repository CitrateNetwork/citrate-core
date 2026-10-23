// created: 2026-10-23 | branch: test/join-e2e-gate | author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
// status: active. Join-E2E harness only (scripts/ci/join-e2e.sh); never shipped, never deployed.
//
// The landing site talks to Postgres through Neon's HTTP SQL endpoint (`neon()` in
// citrate-landing src/lib/db.ts POSTs to `https://<db host>/sql`). The Join-E2E gate runs the landing
// join service against a throwaway local Postgres instead, so this process stands in for that one
// endpoint on loopback:
//
//   https://127.0.0.1:443/sql   (TLS with a throwaway CA the harness makes; the landing process
//                                trusts it through NODE_EXTRA_CA_CERTS)
//        -> a WebSocket-to-unix-socket relay (what Neon's wsproxy does)
//        -> the local postgres, which listens on a unix socket only
//
// Protocol (as @neondatabase/serverless 0.10 speaks it): POST a JSON body `{query, params}` or, for
// a transaction, `{queries: [{query, params}, ...]}` with optional `Neon-Batch-Isolation-Level`,
// `Neon-Batch-Read-Only` and `Neon-Batch-Deferrable` headers. The answer carries `fields`
// (`name`, `dataTypeID`) and `rows` as arrays of raw text values (`Neon-Raw-Text-Output`,
// `Neon-Array-Mode`); the client parses them. A database error is a 400 with the Postgres error
// fields. A transaction is one BEGIN ... COMMIT on one connection, rolled back on any error.
//
// Must run from the landing checkout (it imports the landing's own @neondatabase/serverless and
// ws), as: node neon-http-shim.mjs <pg-socket-path> <cert.pem> <key.pem> <ready-file>
// It refuses any listen address but loopback.

import { createServer } from "node:https";
import { createConnection } from "node:net";
import { readFileSync, writeFileSync } from "node:fs";
import { Pool, neonConfig } from "@neondatabase/serverless";
import ws, { WebSocketServer } from "ws";

const [sock, certFile, keyFile, readyFile] = process.argv.slice(2);
if (!sock || !certFile || !keyFile || !readyFile) {
  console.error("usage: neon-http-shim.mjs <pg-socket> <cert.pem> <key.pem> <ready-file>");
  process.exit(2);
}
const HOST = "127.0.0.1";
const PORT = Number(process.env.JE_SHIM_PORT || 443);

// WebSocket -> unix socket relay on an ephemeral loopback port.
const relay = new WebSocketServer({ host: HOST, port: 0 });
relay.on("connection", (socket) => {
  const pg = createConnection({ path: sock });
  socket.on("message", (data) => pg.write(data));
  pg.on("data", (data) => socket.send(data));
  socket.on("close", () => pg.destroy());
  pg.on("close", () => socket.close());
  pg.on("error", () => socket.close());
  socket.on("error", () => pg.destroy());
});
await new Promise((r) => relay.once("listening", r));
const relayPort = relay.address().port;

neonConfig.webSocketConstructor = ws;
neonConfig.wsProxy = () => `${HOST}:${relayPort}/`;
neonConfig.useSecureWebSocket = false;
neonConfig.pipelineTLS = false;
neonConfig.pipelineConnect = false;

const pools = new Map();
function poolFor(connStr) {
  // One pool per connection string the client names (only the harness's own one is expected). The
  // host part is ignored: everything goes to the local socket through the relay.
  const u = new URL(connStr);
  const key = `${u.username}@${u.pathname}`;
  if (!pools.has(key)) {
    pools.set(
      key,
      new Pool({
        connectionString: `postgresql://${encodeURIComponent(u.username)}@localhost${u.pathname}`,
        max: 20,
      }),
    );
  }
  return pools.get(key);
}

const RAW = { getTypeParser: () => (v) => v };

async function runOne(client, q) {
  const r = await client.query({ text: q.query, values: q.params ?? [], rowMode: "array", types: RAW });
  return {
    command: r.command,
    rowCount: r.rowCount,
    rows: r.rows,
    fields: r.fields.map((f) => ({
      name: f.name,
      dataTypeID: f.dataTypeID,
      tableID: f.tableID,
      columnID: f.columnID,
      dataTypeSize: f.dataTypeSize,
      dataTypeModifier: f.dataTypeModifier,
      format: "text",
    })),
    rowAsArray: true,
  };
}

const ISOLATION = {
  Serializable: "SERIALIZABLE",
  RepeatableRead: "REPEATABLE READ",
  ReadCommitted: "READ COMMITTED",
  ReadUncommitted: "READ UNCOMMITTED",
};

function pgErrorBody(err) {
  const out = { message: err.message };
  for (const k of [
    "severity", "code", "detail", "hint", "position", "internalPosition", "internalQuery",
    "where", "schema", "table", "column", "dataType", "constraint", "file", "line", "routine",
  ]) {
    if (err[k] !== undefined) out[k] = err[k];
  }
  return out;
}

const server = createServer(
  { cert: readFileSync(certFile), key: readFileSync(keyFile) },
  async (req, res) => {
    const reply = (status, body) => {
      res.writeHead(status, { "content-type": "application/json" });
      res.end(JSON.stringify(body));
    };
    if (req.method !== "POST" || new URL(req.url, "https://x").pathname !== "/sql") {
      return reply(404, { message: "not found" });
    }
    const chunks = [];
    for await (const c of req) chunks.push(c);
    let body;
    try {
      body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    } catch {
      return reply(400, { message: "bad json" });
    }
    const connStr = req.headers["neon-connection-string"];
    if (typeof connStr !== "string") return reply(400, { message: "no connection string" });
    let client;
    try {
      client = await poolFor(connStr).connect();
    } catch (err) {
      return reply(500, { message: `connect: ${err.message}` });
    }
    try {
      if (Array.isArray(body.queries)) {
        const iso = ISOLATION[req.headers["neon-batch-isolation-level"]] ?? "READ COMMITTED";
        const ro = req.headers["neon-batch-read-only"] === "true" ? " READ ONLY" : "";
        const def = req.headers["neon-batch-deferrable"] === "true" ? " DEFERRABLE" : "";
        await client.query(`BEGIN ISOLATION LEVEL ${iso}${ro}${def}`);
        try {
          const results = [];
          for (const q of body.queries) results.push(await runOne(client, q));
          await client.query("COMMIT");
          return reply(200, { results });
        } catch (err) {
          await client.query("ROLLBACK").catch(() => {});
          throw err;
        }
      }
      return reply(200, await runOne(client, body));
    } catch (err) {
      return reply(400, pgErrorBody(err));
    } finally {
      client.release();
    }
  },
);

server.listen(PORT, HOST, () => {
  writeFileSync(readyFile, `${HOST}:${server.address().port}\n`);
});
for (const s of ["SIGTERM", "SIGINT"]) {
  process.on(s, async () => {
    server.close();
    relay.close();
    for (const p of pools.values()) await p.end().catch(() => {});
    process.exit(0);
  });
}
