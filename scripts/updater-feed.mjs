#!/usr/bin/env node
// created: 2026-11-08 | branch: ci/release-macos15-arm64-bigint
// author: Larry Klosowski (CORE release-CI lane; Claude Opus 5.5)
// status: active
//
// The in-app updater feed (latest.json). Every installed app reads one feed from two endpoints
// (src-tauri/tauri.conf.json plugins.updater.endpoints; docs/release/053-UPDATER.md), keyed by the
// platform names below. Each release job writes a FRAGMENT for the update files it built and
// signed; the release step merges the fragments into the one latest.json that goes to both
// endpoints, byte-identical (053-UPDATER.md section 3, step 4).
//
// The platform keys and file names are the 0.5.2 feed's. Changing one strands every installed app
// that looks for the old key or file, so the table is the contract and the tests pin it.
//
// Usage:
//   node scripts/updater-feed.mjs fragment --version <x.y.z> --base-url <https URL> --dir <dir> <platform>...
//        print {version, platforms} for the files in <dir>; each <file> needs <file>.sig beside it
//        (what `tauri signer sign` writes), and the url is <base-url>/<file>
//   node scripts/updater-feed.mjs merge --notes <text> --pub-date <RFC 3339 UTC> <fragment.json>...
//        print the feed: {version, notes, pub_date, platforms}; one version, each platform once
//
// Fails closed (exit 1): an unknown platform, a missing file or signature, a signature that is not
// a minisign signature, a base URL that is not https, fragments of different versions, a platform
// twice. Zero dependencies: node built-ins only.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const PLATFORM_FILES = Object.freeze({
  "darwin-aarch64": "Citrate-Core.app.tar.gz",
  "linux-x86_64": "Citrate-Core-linux-x86_64.AppImage",
  "linux-aarch64": "Citrate-Core-linux-aarch64.AppImage",
  "linux-x86_64-deb": "Citrate-Core-linux-x86_64.deb",
  "linux-aarch64-deb": "Citrate-Core-linux-aarch64.deb",
});

const VERSION = /^\d+\.\d+\.\d+$/;
const RFC3339_UTC = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$/;

/** The .sig content (base64 of minisign text), or throw if it is not one. */
function readSignature(file) {
  const sigPath = `${file}.sig`;
  if (!fs.existsSync(sigPath)) throw new Error(`no signature ${sigPath} (sign the file with \`tauri signer sign\`)`);
  const sig = fs.readFileSync(sigPath, "utf8").trim();
  const text = Buffer.from(sig, "base64").toString("utf8");
  if (!sig || !text.startsWith("untrusted comment:") || !/\ntrusted comment: /.test(text)) {
    throw new Error(`${sigPath} is not a minisign signature (expected base64 of a tauri signer signature)`);
  }
  return sig;
}

export function fragment({ version, baseUrl, dir, platforms }) {
  if (!VERSION.test(version || "")) throw new Error(`version must be x.y.z (got '${version}')`);
  if (!/^https:\/\/\S+$/.test(baseUrl || "")) throw new Error(`base URL must be https (got '${baseUrl}')`);
  if (!platforms || platforms.length === 0) throw new Error("name at least one platform");
  const base = baseUrl.replace(/\/+$/, "");
  const out = {};
  for (const p of platforms) {
    const name = PLATFORM_FILES[p];
    if (!name) throw new Error(`unknown platform '${p}' (the apps read: ${Object.keys(PLATFORM_FILES).join(", ")})`);
    if (out[p]) throw new Error(`${p} appears twice`);
    const file = path.join(dir, name);
    if (!fs.existsSync(file)) throw new Error(`no update file ${file} for ${p}`);
    out[p] = { signature: readSignature(file), url: `${base}/${name}` };
  }
  return { version, platforms: out };
}

export function merge({ fragments, notes, pubDate }) {
  if (!fragments || fragments.length === 0) throw new Error("no fragments");
  if (!RFC3339_UTC.test(pubDate || "")) throw new Error(`pub_date must be RFC 3339 UTC, e.g. 2026-11-20T00:00:00Z (got '${pubDate}')`);
  const versions = [...new Set(fragments.map((f) => f.version))];
  if (versions.length !== 1 || !VERSION.test(versions[0] || "")) throw new Error(`fragments name different versions or none: ${versions.join(", ")}`);
  const platforms = {};
  for (const f of fragments) {
    for (const [p, entry] of Object.entries(f.platforms || {})) {
      if (!PLATFORM_FILES[p]) throw new Error(`unknown platform '${p}'`);
      if (platforms[p]) throw new Error(`${p} appears twice`);
      platforms[p] = entry;
    }
  }
  return { version: versions[0], notes: String(notes ?? ""), pub_date: pubDate, platforms };
}

function parseArgs(argv) {
  const opts = {};
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith("--")) {
      if (i + 1 >= argv.length) throw new Error(`${a} needs a value`);
      opts[a.slice(2)] = argv[++i];
    } else rest.push(a);
  }
  return { opts, rest };
}

function main(argv) {
  const [cmd, ...args] = argv;
  const { opts, rest } = parseArgs(args);
  let out;
  if (cmd === "fragment") {
    out = fragment({ version: opts.version, baseUrl: opts["base-url"], dir: opts.dir || ".", platforms: rest });
  } else if (cmd === "merge") {
    const fragments = rest.map((f) => JSON.parse(fs.readFileSync(f, "utf8")));
    out = merge({ fragments, notes: opts.notes, pubDate: opts["pub-date"] });
  } else {
    throw new Error("usage: updater-feed.mjs fragment|merge ... (see the header)");
  }
  process.stdout.write(`${JSON.stringify(out, null, 2)}\n`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    process.stderr.write(`updater-feed: ${e.message}\n`);
    process.exit(1);
  }
}
