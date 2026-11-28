#!/usr/bin/env node
// L6-WP08b: builds the Citrate MCP subregistry (UNSIGNED) from the official MCP Registry and the
// reviewed overlay `curation.json`, plus a human diff for the review PR.
//
// Data source (Rule 7): GET https://registry.modelcontextprotocol.io/v0.1/servers (cursor
// pagination, latest versions only) and tools/mcp-mirror/curation.json. Smithery is not a source.
//
// Rules (the app's verifier, components/src/mcp_subregistry.rs, enforces the same and refuses the
// whole document on any break):
// - Only names in the overlay are mirrored, each with its Citrate curation record
//   (`_meta["ai.citrate/curation"] = { trust, reviewedAt, toolSnapshotSha256? }`).
// - Only the reviewed remote URLs are mirrored, and only https ones with a streamable-http or sse
//   transport. If a reviewed URL is no longer upstream, the entry is dropped and listed in the
//   diff: curation never follows a moved server silently.
// - Packages must match the reviewed registry type, identifier and exact version; a bump or
//   `latest` or a range drops the entry until it is reviewed again.
// - Output is deterministic: servers sorted by name, fixed key order, no clock read unless
//   --issued-at is left out.
//
// This tool has no key path and does not sign. The owner signs the output offline with the
// catalog key (docs/MCP_MARKETPLACE.md).
//
// Usage:
//   node tools/mcp-mirror/build.mjs --sequence N --out DIR [--issued-at UNIX] [--previous FILE]
//        [--curation FILE] [--from-file UPSTREAM.json]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const REGISTRY_BASE = "https://registry.modelcontextprotocol.io";
export const SCHEMA = 1;
export const LIFETIME_SECS = 14 * 86400;
const MAX_LIFETIME_SECS = 31 * 86400;
const MAX_PAGE_BYTES = 8 << 20;
const MAX_PAGES = 500;
const TRUST = new Set(["verified_brand", "curated"]);
const TRANSPORTS = new Set(["streamable-http", "sse"]);
const REGISTRY_TYPES = new Set(["npm", "pypi", "oci"]);
const NAME_RE = /^[A-Za-z0-9][A-Za-z0-9.-]*\/[A-Za-z0-9._-]+$/;
const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;
const SHA_RE = /^[0-9a-f]{64}$/;
const PINNED_RE = /^[A-Za-z0-9][A-Za-z0-9.+_-]{0,63}$/;
const IDENT_RE = /^[A-Za-z0-9@][A-Za-z0-9@/._:-]{0,213}$/;

/** An https URL with a host, printable ASCII, no credentials and no `{template}` parts. */
export function isHttpsUrl(u) {
  if (typeof u !== "string" || u.length > 2048 || !/^[\x21-\x7e]+$/.test(u)) return false;
  if (!u.startsWith("https://") || /[{}]/.test(u)) return false;
  const host = u.slice(8).split(/[/?#]/)[0];
  return host.length > 0 && !host.includes("@");
}

function isPinned(v) {
  return typeof v === "string" && PINNED_RE.test(v) && v.toLowerCase() !== "latest";
}

function isText(s, max) {
  // eslint-disable-next-line no-control-regex
  return typeof s === "string" && s.trim().length > 0 && [...s].length <= max && !/[\x00-\x09\x0b-\x1f\x7f]/.test(s);
}

/**
 * Every page of a registry listing, following `metadata.nextCursor`; latest active versions only.
 * `search` narrows the listing (the registry's substring match on the name).
 */
export async function fetchAllServers(fetchImpl, base = REGISTRY_BASE, { search } = {}) {
  const out = [];
  const seen = new Set();
  let cursor = null;
  for (let page = 0; page < MAX_PAGES; page++) {
    const q = new URLSearchParams({ limit: "100" });
    if (search) q.set("search", search);
    if (cursor) q.set("cursor", cursor);
    const url = `${base}/v0.1/servers?${q}`;
    const res = await fetchImpl(url);
    if (!res.ok) throw new Error(`registry ${url}: HTTP ${res.status}`);
    const text = await res.text();
    if (text.length > MAX_PAGE_BYTES) throw new Error(`registry page larger than ${MAX_PAGE_BYTES} bytes`);
    const body = JSON.parse(text);
    for (const it of body.servers ?? []) {
      const official = it?._meta?.["io.modelcontextprotocol.registry/official"] ?? {};
      if (it?.server && official.isLatest !== false && (official.status ?? "active") === "active") out.push(it.server);
    }
    const next = body.metadata?.nextCursor;
    if (!next) return out;
    if (seen.has(next)) throw new Error(`registry cursor repeated: ${next}`);
    seen.add(next);
    cursor = next;
  }
  throw new Error(`registry has more than ${MAX_PAGES} pages`);
}

function checkCuration(curation) {
  for (const [name, c] of Object.entries(curation?.entries ?? {})) {
    if (!NAME_RE.test(name)) throw new Error(`curation: name ${JSON.stringify(name)}`);
    if (!TRUST.has(c.trust)) throw new Error(`curation ${name}: trust ${JSON.stringify(c.trust)}`);
    if (!DATE_RE.test(c.reviewedAt ?? "")) throw new Error(`curation ${name}: reviewedAt`);
    if (!isText(c.title, 80) || c.title.includes("\n")) throw new Error(`curation ${name}: title`);
    if (c.toolSnapshotSha256 !== undefined && !SHA_RE.test(c.toolSnapshotSha256)) {
      throw new Error(`curation ${name}: toolSnapshotSha256`);
    }
    if (!(c.remoteUrls?.length || c.packages?.length)) throw new Error(`curation ${name}: no reviewed remote or package`);
  }
}

const samePackage = (a, b) =>
  a.registryType === b.registryType && a.identifier === b.identifier && a.version === b.version;

/** The upstream records of the curated names only: one search per name, exact matches kept. */
export async function fetchCuratedServers(fetchImpl, names, base = REGISTRY_BASE) {
  const out = [];
  for (const name of [...names].sort()) {
    const found = await fetchAllServers(fetchImpl, base, { search: name });
    out.push(...found.filter((s) => s.name === name));
  }
  return out;
}

/** The mirrored entry for one upstream server, or `{ reason }` when it is dropped. */
function mirrorEntry(up, c) {
  if (!isText(up.description, 600)) return { reason: "description missing or too long" };
  if (!isPinned(up.version)) return { reason: `version ${JSON.stringify(up.version)}` };
  const upRemotes = (up.remotes ?? []).filter((r) => TRANSPORTS.has(r.type) && isHttpsUrl(r.url));
  const remotes = [];
  for (const url of c.remoteUrls ?? []) {
    if (!isHttpsUrl(url)) return { reason: `reviewed remote ${url} is not https` };
    const r = upRemotes.find((x) => x.url === url);
    if (!r) return { reason: `remote URL changed (reviewed ${url} is no longer listed upstream)` };
    remotes.push({ type: r.type, url: r.url });
  }
  const packages = [];
  for (const p of c.packages ?? []) {
    if (!REGISTRY_TYPES.has(p.registryType) || !IDENT_RE.test(p.identifier) || p.identifier.includes("..")) {
      return { reason: `reviewed package ${p.identifier} is not a supported registry package` };
    }
    if (!isPinned(p.version)) return { reason: `reviewed package ${p.identifier} is not pinned to an exact version` };
    if (!(up.packages ?? []).some((x) => samePackage(x, p))) {
      return { reason: `package changed (reviewed ${p.identifier}@${p.version} is no longer listed upstream)` };
    }
    packages.push({ registryType: p.registryType, identifier: p.identifier, version: p.version });
  }
  if (!remotes.length && !packages.length) return { reason: "no https remote or pinned package" };
  const curation = { trust: c.trust, reviewedAt: c.reviewedAt };
  if (c.toolSnapshotSha256) curation.toolSnapshotSha256 = c.toolSnapshotSha256;
  return {
    entry: {
      name: up.name,
      title: c.title,
      description: up.description,
      version: up.version,
      remotes,
      packages,
      _meta: { "ai.citrate/curation": curation },
    },
  };
}

/**
 * Builds the document from upstream `server` objects and the overlay. Returns the document and
 * the entries dropped with their reason (curated names missing upstream are dropped too).
 */
export function buildDocument(upstream, curation, { sequence, issuedAt, previous = null, lifetime = LIFETIME_SECS }) {
  checkCuration(curation);
  if (!Number.isSafeInteger(sequence) || sequence < 1) throw new Error("sequence must be a positive integer");
  if (previous && sequence <= previous.sequence) {
    throw new Error(`sequence ${sequence} must be above the previous document's ${previous.sequence}`);
  }
  if (!Number.isSafeInteger(issuedAt) || issuedAt < 0) throw new Error("issuedAt must be unix seconds");
  if (!(lifetime > 0 && lifetime <= MAX_LIFETIME_SECS)) throw new Error("lifetime out of bounds");
  const entries = curation.entries ?? {};
  const byName = new Map();
  for (const s of upstream) if (s?.name && !byName.has(s.name)) byName.set(s.name, s);
  const servers = [];
  const dropped = [];
  for (const name of Object.keys(entries).sort()) {
    const up = byName.get(name);
    if (!up) {
      dropped.push({ name, reason: "not listed upstream" });
      continue;
    }
    const r = mirrorEntry(up, entries[name]);
    if (r.entry) servers.push(r.entry);
    else dropped.push({ name, reason: r.reason });
  }
  return {
    document: { schema: SCHEMA, sequence, issued_at: issuedAt, expires_at: issuedAt + lifetime, servers },
    dropped,
  };
}

/** The exact bytes the owner signs: fixed key order, two-space indent, trailing newline. */
export function renderDocument(doc) {
  const ordered = {
    schema: doc.schema,
    sequence: doc.sequence,
    issued_at: doc.issued_at,
    expires_at: doc.expires_at,
    servers: [...doc.servers]
      .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))
      .map((s) => ({
        name: s.name,
        title: s.title,
        description: s.description,
        version: s.version,
        remotes: s.remotes.map((r) => ({ type: r.type, url: r.url })),
        packages: s.packages.map((p) => ({ registryType: p.registryType, identifier: p.identifier, version: p.version })),
        _meta: { "ai.citrate/curation": s._meta["ai.citrate/curation"] },
      })),
  };
  return `${JSON.stringify(ordered, null, 2)}\n`;
}

/** A human diff: `+` added, `-` removed, `~` changed, `!` dropped with its reason. */
export function diffDocuments(prev, next, dropped = []) {
  const p = new Map((prev?.servers ?? []).map((s) => [s.name, JSON.stringify(s)]));
  const n = new Map(next.servers.map((s) => [s.name, JSON.stringify(s)]));
  const lines = [`sequence ${prev?.sequence ?? "none"} -> ${next.sequence}`];
  for (const name of [...new Set([...p.keys(), ...n.keys()])].sort()) {
    if (!p.has(name)) lines.push(`+ ${name}`);
    else if (!n.has(name)) lines.push(`- ${name}`);
    else if (p.get(name) !== n.get(name)) lines.push(`~ ${name}`);
  }
  for (const d of dropped) lines.push(`! ${d.name}: ${d.reason}`);
  return `${lines.join("\n")}\n`;
}

function arg(argv, flag) {
  const i = argv.indexOf(flag);
  return i >= 0 ? argv[i + 1] : undefined;
}

async function main(argv) {
  const here = path.dirname(fileURLToPath(import.meta.url));
  const out = arg(argv, "--out");
  const sequence = Number(arg(argv, "--sequence"));
  if (!out || !Number.isSafeInteger(sequence)) {
    throw new Error("usage: build.mjs --sequence N --out DIR [--issued-at UNIX] [--previous FILE] [--curation FILE] [--from-file FILE]");
  }
  const issuedRaw = arg(argv, "--issued-at");
  const issuedAt = issuedRaw === undefined ? Math.floor(Date.now() / 1000) : Number(issuedRaw);
  const curation = JSON.parse(fs.readFileSync(arg(argv, "--curation") ?? path.join(here, "curation.json"), "utf8"));
  const prevPath = arg(argv, "--previous");
  const previous = prevPath ? JSON.parse(fs.readFileSync(prevPath, "utf8")) : null;
  const fromFile = arg(argv, "--from-file");
  const upstream = fromFile
    ? (JSON.parse(fs.readFileSync(fromFile, "utf8")).servers ?? []).map((it) => it.server ?? it)
    : await fetchCuratedServers(
        (u) => fetch(u, { redirect: "error", signal: AbortSignal.timeout(30_000) }),
        Object.keys(curation.entries ?? {}),
      );
  const { document, dropped } = buildDocument(upstream, curation, { sequence, issuedAt, previous });
  fs.mkdirSync(out, { recursive: true });
  fs.writeFileSync(path.join(out, "mcp-subregistry.json"), renderDocument(document));
  fs.writeFileSync(path.join(out, "mcp-subregistry.diff.txt"), diffDocuments(previous, document, dropped));
  process.stdout.write(`wrote ${document.servers.length} servers (${dropped.length} dropped) to ${out}; unsigned\n`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((e) => {
    process.stderr.write(`mcp-mirror: ${e.message}\n`);
    process.exit(1);
  });
}
