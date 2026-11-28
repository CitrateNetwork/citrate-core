// @vitest-environment node
// L6-WP08b: the Citrate subregistry builder. It reads the official MCP Registry and the reviewed
// overlay (curation.json) and writes the UNSIGNED subregistry document plus a human diff. It never
// signs: the owner signs offline with the catalog key.
//
// Run: npx vitest run tools/mcp-mirror (CI picks it up once vitest.config.ts includes
// tools/**/*.test.mjs, the L0 seam).
import { test } from "vitest";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import {
  REGISTRY_BASE,
  fetchAllServers,
  fetchCuratedServers,
  buildDocument,
  renderDocument,
  diffDocuments,
} from "../build.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TOOL = path.join(HERE, "..", "build.mjs");
const ISSUED = 1_795_000_000;

function item(name, { remotes = [], packages = [], version = "1.0.0", title, description = "A server." } = {}) {
  return {
    server: { name, ...(title ? { title } : {}), description, version, remotes, packages },
    _meta: { "io.modelcontextprotocol.registry/official": { status: "active", isLatest: true } },
  };
}

// A fake registry: pages keyed by cursor (null = first page). Records every URL asked for.
function fakeFetch(pages) {
  const asked = [];
  const impl = async (url) => {
    asked.push(url);
    const cursor = new URL(url).searchParams.get("cursor");
    const page = pages[cursor ?? ""];
    if (!page) return { ok: false, status: 404, text: async () => "not found" };
    return { ok: true, status: 200, text: async () => JSON.stringify(page) };
  };
  return { impl, asked };
}

const NOTION = item("com.notion/mcp", {
  remotes: [{ type: "streamable-http", url: "https://mcp.notion.com/mcp" }],
});
const FILES = item("io.github.example/files", {
  packages: [{ registryType: "npm", identifier: "@example/files-mcp", version: "0.4.2" }],
});

const CURATION = {
  entries: {
    "com.notion/mcp": {
      title: "Notion",
      trust: "verified_brand",
      reviewedAt: "2026-11-28",
      remoteUrls: ["https://mcp.notion.com/mcp"],
    },
    "io.github.example/files": {
      title: "Example files",
      trust: "curated",
      reviewedAt: "2026-11-28",
      packages: [{ registryType: "npm", identifier: "@example/files-mcp", version: "0.4.2" }],
      toolSnapshotSha256: "ab".repeat(32),
    },
  },
};

test("paginates_with_cursor", async () => {
  const { impl, asked } = fakeFetch({
    "": { servers: [NOTION], metadata: { nextCursor: "c2", count: 1 } },
    c2: { servers: [FILES], metadata: { nextCursor: "c3", count: 1 } },
    c3: { servers: [], metadata: { count: 0 } },
  });
  const all = await fetchAllServers(impl, REGISTRY_BASE);
  assert.deepEqual(
    all.map((s) => s.name),
    ["com.notion/mcp", "io.github.example/files"],
  );
  assert.equal(asked.length, 3);
  assert.ok(asked[0].startsWith("https://registry.modelcontextprotocol.io/v0.1/servers?"), asked[0]);
  assert.equal(new URL(asked[1]).searchParams.get("cursor"), "c2");
});

test("curated fetch searches each name and keeps exact matches only", async () => {
  const asked = [];
  const impl = async (url) => {
    asked.push(url);
    const u = new URL(url);
    const search = u.searchParams.get("search");
    const servers =
      search === "com.notion/mcp"
        ? [NOTION, item("com.notion/mcp-lookalike", { remotes: [{ type: "streamable-http", url: "https://x.example/mcp" }] })]
        : [FILES];
    return { ok: true, status: 200, text: async () => JSON.stringify({ servers, metadata: {} }) };
  };
  const got = await fetchCuratedServers(impl, ["io.github.example/files", "com.notion/mcp"], REGISTRY_BASE);
  assert.deepEqual(got.map((s) => s.name), ["com.notion/mcp", "io.github.example/files"]);
  assert.equal(new URL(asked[0]).searchParams.get("search"), "com.notion/mcp");
});

test("an inactive or older version upstream is not mirrored", async () => {
  const old = { ...NOTION, _meta: { "io.modelcontextprotocol.registry/official": { status: "active", isLatest: false } } };
  const gone = { ...FILES, _meta: { "io.modelcontextprotocol.registry/official": { status: "deleted", isLatest: true } } };
  const { impl } = fakeFetch({ "": { servers: [old, gone], metadata: {} } });
  assert.deepEqual(await fetchAllServers(impl, REGISTRY_BASE), []);
});

test("pagination stops on a repeated cursor and on a page limit", async () => {
  const loop = fakeFetch({
    "": { servers: [NOTION], metadata: { nextCursor: "a" } },
    a: { servers: [], metadata: { nextCursor: "a" } },
  });
  await assert.rejects(fetchAllServers(loop.impl, REGISTRY_BASE), /cursor repeated/);
  const bad = fakeFetch({});
  await assert.rejects(fetchAllServers(bad.impl, REGISTRY_BASE), /404/);
});

test("an oversized registry page is refused", async () => {
  const impl = async () => ({ ok: true, status: 200, text: async () => "x".repeat(9 << 20) });
  await assert.rejects(fetchAllServers(impl, REGISTRY_BASE), /larger than/);
});

test("keeps_only_entries_with_https_remotes_or_pinned_packages", () => {
  const upstream = [
    NOTION.server,
    item("com.example/plain-http", { remotes: [{ type: "streamable-http", url: "http://example.com/mcp" }] }).server,
    item("com.example/latest", {
      packages: [{ registryType: "npm", identifier: "@example/latest", version: "latest" }],
    }).server,
    item("com.example/range", {
      packages: [{ registryType: "pypi", identifier: "example", version: "^1.2" }],
    }).server,
    item("com.example/ws", { remotes: [{ type: "websocket", url: "https://example.com/ws" }] }).server,
  ];
  const curation = { entries: { ...CURATION.entries } };
  for (const n of ["com.example/plain-http", "com.example/latest", "com.example/range", "com.example/ws"]) {
    curation.entries[n] = { title: n, trust: "curated", reviewedAt: "2026-11-28" };
  }
  curation.entries["com.example/plain-http"].remoteUrls = ["http://example.com/mcp"];
  curation.entries["com.example/ws"].remoteUrls = ["https://example.com/ws"];
  curation.entries["com.example/latest"].packages = [{ registryType: "npm", identifier: "@example/latest", version: "latest" }];
  curation.entries["com.example/range"].packages = [{ registryType: "pypi", identifier: "example", version: "^1.2" }];
  const { document, dropped } = buildDocument(upstream, curation, { sequence: 1, issuedAt: ISSUED });
  assert.deepEqual(
    document.servers.map((s) => s.name),
    ["com.notion/mcp"],
  );
  for (const n of ["com.example/plain-http", "com.example/latest", "com.example/range", "com.example/ws"]) {
    assert.ok(dropped.some((d) => d.name === n), `${n} dropped`);
  }
  for (const s of document.servers) {
    for (const r of s.remotes) assert.ok(r.url.startsWith("https://"));
  }
});

test("only curated names are mirrored, with the Citrate curation record", () => {
  const upstream = [NOTION.server, FILES.server, item("com.unknown/server", {
    remotes: [{ type: "streamable-http", url: "https://unknown.example/mcp" }],
  }).server];
  const { document } = buildDocument(upstream, CURATION, { sequence: 3, issuedAt: ISSUED });
  assert.deepEqual(
    document.servers.map((s) => s.name),
    ["com.notion/mcp", "io.github.example/files"],
  );
  const notion = document.servers[0];
  assert.deepEqual(notion._meta, {
    "ai.citrate/curation": { trust: "verified_brand", reviewedAt: "2026-11-28" },
  });
  assert.equal(notion.title, "Notion");
  const files = document.servers[1];
  assert.equal(files._meta["ai.citrate/curation"].toolSnapshotSha256, "ab".repeat(32));
  assert.deepEqual(files.packages, [{ registryType: "npm", identifier: "@example/files-mcp", version: "0.4.2" }]);
  assert.equal(document.schema, 1);
  assert.equal(document.sequence, 3);
  assert.equal(document.issued_at, ISSUED);
  assert.ok(document.expires_at > ISSUED && document.expires_at - ISSUED <= 31 * 86400);
});

test("url_change_drops_trust", () => {
  const moved = item("com.notion/mcp", {
    remotes: [{ type: "streamable-http", url: "https://mcp.notion-new.example/mcp" }],
  }).server;
  const { document, dropped } = buildDocument([moved, FILES.server], CURATION, { sequence: 2, issuedAt: ISSUED });
  assert.ok(!document.servers.some((s) => s.name === "com.notion/mcp"), "a moved server is not mirrored");
  const d = dropped.find((x) => x.name === "com.notion/mcp");
  assert.match(d.reason, /remote URL changed/);
  // A package version bump is the same: it needs a new review.
  const bumped = item("io.github.example/files", {
    packages: [{ registryType: "npm", identifier: "@example/files-mcp", version: "0.5.0" }],
  }).server;
  const r2 = buildDocument([NOTION.server, bumped], CURATION, { sequence: 2, issuedAt: ISSUED });
  assert.match(r2.dropped.find((x) => x.name === "io.github.example/files").reason, /package changed/);
});

test("a curation record with an unknown trust value is refused", () => {
  const bad = { entries: { "com.notion/mcp": { ...CURATION.entries["com.notion/mcp"], trust: "registry_unreviewed" } } };
  assert.throws(() => buildDocument([NOTION.server], bad, { sequence: 1, issuedAt: ISSUED }), /trust/);
});

test("output_is_deterministic", () => {
  const a = renderDocument(
    buildDocument([FILES.server, NOTION.server], CURATION, { sequence: 5, issuedAt: ISSUED }).document,
  );
  const b = renderDocument(
    buildDocument([NOTION.server, FILES.server], CURATION, { sequence: 5, issuedAt: ISSUED }).document,
  );
  assert.equal(a, b);
  assert.ok(a.endsWith("\n"));
  // Key order is fixed, so the bytes the owner signs never depend on the upstream order.
  assert.ok(a.indexOf('"schema"') < a.indexOf('"sequence"'));
  assert.ok(a.indexOf('"sequence"') < a.indexOf('"servers"'));
});

test("the diff names added, removed and changed servers", () => {
  const prev = buildDocument([NOTION.server], CURATION, { sequence: 1, issuedAt: ISSUED }).document;
  const next = buildDocument([FILES.server], CURATION, { sequence: 2, issuedAt: ISSUED }).document;
  const text = diffDocuments(prev, next, [{ name: "com.notion/mcp", reason: "remote URL changed" }]);
  assert.match(text, /^\+ io\.github\.example\/files/m);
  assert.match(text, /^- com\.notion\/mcp/m);
  assert.match(text, /^! com\.notion\/mcp: remote URL changed/m);
});

test("the sequence must go up from the previous document", () => {
  const prev = buildDocument([NOTION.server], CURATION, { sequence: 4, issuedAt: ISSUED }).document;
  assert.throws(
    () => buildDocument([NOTION.server], CURATION, { sequence: 4, issuedAt: ISSUED, previous: prev }),
    /sequence/,
  );
});

test("never_signs", () => {
  const src = fs.readFileSync(TOOL, "utf8");
  for (const word of ["minisign", "createSign", "privateKey", "secretKey", "signSync", ".minisig", "generateKeyPair"]) {
    assert.ok(!src.includes(word), `build.mjs mentions ${word}`);
  }
  // The CLI writes the document and the diff, nothing else.
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "mcp-mirror-"));
  try {
    const up = path.join(dir, "upstream.json");
    fs.writeFileSync(up, JSON.stringify({ servers: [NOTION, FILES] }));
    const cur = path.join(dir, "curation.json");
    fs.writeFileSync(cur, JSON.stringify(CURATION));
    const out = path.join(dir, "out");
    execFileSync(process.execPath, [
      TOOL, "--from-file", up, "--curation", cur, "--sequence", "1", "--issued-at", String(ISSUED), "--out", out,
    ]);
    assert.deepEqual(fs.readdirSync(out).sort(), ["mcp-subregistry.diff.txt", "mcp-subregistry.json"]);
    const doc = JSON.parse(fs.readFileSync(path.join(out, "mcp-subregistry.json"), "utf8"));
    assert.equal(doc.servers.length, 2);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("the checked-in curation overlay is valid", () => {
  const cur = JSON.parse(fs.readFileSync(path.join(HERE, "..", "curation.json"), "utf8"));
  // Building over an empty upstream validates every record without network.
  assert.doesNotThrow(() => buildDocument([], cur, { sequence: 1, issuedAt: ISSUED }));
  for (const [name, c] of Object.entries(cur.entries)) {
    assert.match(name, /^[A-Za-z0-9][A-Za-z0-9.-]*\/[A-Za-z0-9._-]+$/);
    assert.ok(["verified_brand", "curated"].includes(c.trust), name);
  }
});
