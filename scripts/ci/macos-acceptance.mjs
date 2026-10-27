#!/usr/bin/env node
// created: 2026-10-27 | branch: rc52/macos-acceptance | author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
// status: active (RC52 WP-H-MACOS, gate G1 g1-macos)
// =====================================================================
// citrate-core: macOS packaged acceptance of the RC DMG (port of the K1-L Linux checks)
//
//   node scripts/ci/macos-acceptance.mjs --dmg Citrate.dmg --sha256 <hex> [--mode inspect|verify|full]
//        [--pinned-dir DIR] [--team-id ID] [--expect-version 0.5.2] [--work DIR] [--log-dir DIR]
//        [--out verdict.json]
//   node scripts/ci/macos-acceptance.mjs --self-test [--work DIR] [--log-dir DIR]
//
// TEST MACHINE ONLY. It refuses (exit 2) before it runs any command unless BOTH hold:
//   - CITRATE_MACOS_ACCEPTANCE_TEST_MACHINE=1 is set, and
//   - this machine's hostname is a line of ~/.config/citrate/macos-acceptance-test-hosts (under the
//     account's real home from the password database), a file owned by you and not group- or
//     world-writable.
// It then probes (read-only) and refuses if any of these holds: not macOS; root; Citrate Core is
// installed, has data under the real home, or runs; a node port (8545, 8546, 30303) is in use; the
// keychain holds an ai.citrate.core item; a "Developer ID Application" signing identity is in the
// keychain (that is a release machine). A probe that cannot run counts as a refusal.
//
// Modes (the default is the dry run):
//   inspect  reads the DMG file only: sha256, image checksum, format, DMG signature. No mount.
//   verify   inspect + mounts the DMG read-only (no copy, no launch): bundle identity and layout,
//            sidecar digests against release/components.json, code signatures, Gatekeeper,
//            notarization ticket.
//   full     verify + installs into a run directory (never /Applications), launches with an isolated
//            HOME and a throwaway keychain, and checks ready, restart, quit, reap, uninstall. Only a
//            full run that passes is G1 evidence (verdict field g1_evidence).
//
// Verdict: JSON (schema citrate.macos-acceptance.v2) at --out, default <log-dir>/verdict.json.
// overall: pass | pass-unsigned-smoke (self-test only) | fail | error | refused.
// Exit 0 pass / pass-unsigned-smoke, 1 fail, 2 error or refused.
//
// Env: MACOS_ACC_LAUNCH_TIMEOUT (s, default 240), MACOS_ACC_RESTART_TIMEOUT (120),
//      MACOS_ACC_QUIT_TIMEOUT (60), MACOS_ACC_SETTLE_SECS (15), MACOS_ACC_KEEP=1 keeps the run dir.
// Doc: docs/MACOS_ACCEPTANCE.md. No dependencies beyond Node's standard library.
// =====================================================================
import { spawn, spawnSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { loadManifest, parsePins, pinnedDigest } from "../component-pins.mjs";

export const SCHEMA = "citrate.macos-acceptance.v2";
export const BUNDLE_ID = "ai.citrate.core";
export const APP_DIR_NAME = "Citrate Core.app";
export const KEYCHAIN_SERVICES = ["ai.citrate.core", "ai.citrate.core.custody"];
export const NODE_PORTS = [8545, 8546, 30303];
export const EXPECTED_CHAIN_ID = 40204;
export const TARGET = "aarch64-apple-darwin";
export const MARKER_ENV = "CITRATE_MACOS_ACCEPTANCE_TEST_MACHINE";
export const MODES = ["inspect", "verify", "full"];
const HERE = path.dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = path.resolve(HERE, "..", "..");
export const DATA_DIR = path.join(HERE, "macos-acceptance");
// The overlay release.yml bundles with (a tripwire in the tests keeps the two in step).
export const RELEASE_OVERLAY = "src-tauri/tauri.bundle-lite.conf.json";

export class UsageError extends Error {}

// ---------------------------------------------------------------- arguments
export function parseArgs(argv) {
  const o = {
    dmg: "", sha256: "", mode: "full", pinnedDir: "", teamId: "", expectVersion: "", unsignedSmoke: false,
    selfTest: false, work: "", logDir: "", out: "",
    quitMethod: "apple-event", required: ["citrate"], restartTarget: "citrate", rpc: true,
  };
  const val = (i) => {
    if (i + 1 >= argv.length || argv[i + 1].startsWith("--")) throw new UsageError(`${argv[i]} needs a value`);
    return argv[i + 1];
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    switch (a) {
      case "--dmg": o.dmg = val(i); i++; break;
      case "--sha256": o.sha256 = val(i).toLowerCase(); i++; break;
      case "--mode": o.mode = val(i); i++; break;
      case "--pinned-dir": o.pinnedDir = val(i); i++; break;
      case "--team-id": o.teamId = val(i); i++; break;
      case "--expect-version": o.expectVersion = val(i); i++; break;
      case "--work": o.work = val(i); i++; break;
      case "--log-dir": o.logDir = val(i); i++; break;
      case "--out": o.out = val(i); i++; break;
      case "--required": o.required = val(i).split(",").filter(Boolean); i++; break;
      case "--restart-target": o.restartTarget = val(i); i++; break;
      case "--self-test": o.selfTest = true; break;
      default: throw new UsageError(`unknown argument: ${a}`);
    }
  }
  if (!MODES.includes(o.mode)) throw new UsageError(`--mode must be one of ${MODES.join(", ")}`);
  if (!o.selfTest) {
    if (!o.dmg) throw new UsageError("--dmg is required");
    if (!/^[0-9a-f]{64}$/.test(o.sha256)) throw new UsageError("--sha256 <64 hex> is required");
  }
  if (o.required.length === 0) throw new UsageError("--required needs at least one sidecar");
  return o;
}

// ---------------------------------------------------------------- parsers (pure)
/** `ps -axww -o pid=,ppid=,comm=` -> [{pid, ppid, path}] (macOS comm is the full executable path). */
export function parsePs(text) {
  const out = [];
  for (const line of text.split("\n")) {
    const m = /^\s*(\d+)\s+(\d+)\s+(.+?)\s*$/.exec(line);
    if (m) out.push({ pid: Number(m[1]), ppid: Number(m[2]), path: m[3] });
  }
  return out;
}

/** Every pid below `root` (not including it) in a ps snapshot. */
export function descendants(procs, root) {
  const kids = new Map();
  for (const p of procs) {
    if (!kids.has(p.ppid)) kids.set(p.ppid, []);
    kids.get(p.ppid).push(p.pid);
  }
  const seen = new Set();
  const stack = [...(kids.get(root) || [])];
  while (stack.length) {
    const pid = stack.pop();
    if (seen.has(pid) || pid === root) continue;
    seen.add(pid);
    stack.push(...(kids.get(pid) || []));
  }
  return seen;
}

/** True when `p` lies inside directory `dir` (not a sibling with a common prefix). */
export function isInside(p, dir) {
  const d = dir.endsWith("/") ? dir : dir + "/";
  return p.startsWith(d);
}

/** Signals go only to a process whose executable is inside the app this run installed, or to a pid
 *  this run started or saw below one it started, and only while that pid still runs the executable
 *  it ran when it was recorded (a reused pid is not ours). Anything else is refused. */
export function mayKill(proc, installApp, ownPids) {
  if (!proc || !Number.isInteger(proc.pid) || proc.pid <= 1) return false;
  return isInside(proc.path, installApp) || ownPids.get(proc.pid) === proc.path;
}

/** `lsof -nP -iTCP -sTCP:LISTEN -iUDP -F pcPn` -> [{pid, command, proto, local}] */
export function parseLsof(text) {
  const out = [];
  let pid = 0, command = "", proto = "";
  for (const line of text.split("\n")) {
    if (!line) continue;
    const t = line[0], v = line.slice(1);
    if (t === "p") { pid = Number(v); command = ""; proto = ""; }
    else if (t === "c") command = v;
    else if (t === "P") proto = v.toLowerCase();
    else if (t === "n") out.push({ pid, command, proto, local: v.split("->")[0] });
  }
  return out;
}

/** Split "host:port" ("[::1]:8545", "*:30303", "127.0.0.1:5001") into {host, port}. */
export function splitHostPort(local) {
  const m = /^(.*):(\d+|\*)$/.exec(local);
  if (!m) return { host: local, port: NaN };
  return { host: m[1].replace(/^\[|\]$/g, ""), port: m[2] === "*" ? NaN : Number(m[2]) };
}

export function classifyAddress(local) {
  const { host } = splitHostPort(local);
  if (host === "*" || host === "0.0.0.0" || host === "::") return "wildcard";
  if (/^127\.\d+\.\d+\.\d+$/.test(host) || host === "::1" || host === "localhost") return "loopback";
  return "external";
}

/** Allowlist files: one entry per line, whitespace-separated columns, `#` comments. */
export function parseAllowlist(text, columns) {
  const rows = [];
  for (const raw of text.split("\n")) {
    const line = raw.replace(/#.*/, "").trim();
    if (!line) continue;
    const parts = line.split(/\s+/);
    if (parts.length < columns.length) throw new Error(`allowlist line needs ${columns.join(" ")}: ${raw}`);
    const row = {};
    columns.forEach((c, i) => { row[c] = i === columns.length - 1 ? parts.slice(i).join(" ") : parts[i]; });
    rows.push(row);
  }
  return rows;
}

/** Listening sockets of the app's processes: every one must be loopback, or a documented
 *  wildcard bind (exe basename + proto + port in wildcard-allowlist.txt). External binds fail. */
export function evaluateListeners(listeners, procPathByPid, allow) {
  const bad = [], allowed = [], loopback = [];
  for (const l of listeners) {
    const kind = classifyAddress(l.local);
    const exe = path.basename(procPathByPid.get(l.pid) || l.command || "");
    const { port } = splitHostPort(l.local);
    const row = { exe, pid: l.pid, proto: l.proto, local: l.local, kind };
    if (kind === "loopback") { loopback.push(row); continue; }
    const doc = kind === "wildcard" && allow.find((a) => a.exe === exe && a.proto === l.proto && Number(a.port) === port);
    if (doc) allowed.push({ ...row, reason: doc.reason });
    else bad.push(row);
  }
  return { ok: bad.length === 0, bad, allowed, loopback };
}

/** `codesign -dvvv` (stderr) -> {identifier, teamId, authorities, runtime, adhoc} */
export function parseCodesignDisplay(text) {
  const get = (k) => { const m = new RegExp(`^${k}=(.*)$`, "m").exec(text); return m ? m[1].trim() : ""; };
  const authorities = [...text.matchAll(/^Authority=(.*)$/gm)].map((m) => m[1].trim());
  const flags = /^CodeDirectory .*flags=0x[0-9a-f]+\(([^)]*)\)/m.exec(text);
  const flagList = flags ? flags[1].split(",") : [];
  const team = get("TeamIdentifier");
  return {
    identifier: get("Identifier"),
    teamId: team === "not set" ? "" : team,
    authorities,
    runtime: flagList.includes("runtime"),
    adhoc: flagList.includes("adhoc") || /^Signature=adhoc$/m.test(text),
  };
}

/** Developer ID release signature: Developer ID Application authority, team id (matching the
 *  expected one when given), hardened runtime, not ad hoc. */
export function evaluateDeveloperId(sig, expectedTeam, { requireRuntime = true } = {}) {
  const why = [];
  if (sig.adhoc) why.push("ad hoc signature");
  if (!sig.authorities.some((a) => a.startsWith("Developer ID Application:"))) why.push("no Developer ID Application authority");
  if (!sig.teamId) why.push("no team identifier");
  if (expectedTeam && sig.teamId !== expectedTeam) why.push(`team ${sig.teamId || "none"} != expected ${expectedTeam}`);
  if (requireRuntime && !sig.runtime) why.push("hardened runtime flag missing");
  return { ok: why.length === 0, why };
}

/** `spctl --assess -vv` exit code + stderr -> {accepted, source} */
export function parseSpctl(text, code) {
  const src = /^source=(.*)$/m.exec(text);
  const accepted = code === 0 && /: accepted$/m.test(text);
  return { accepted, source: src ? src[1].trim() : "" };
}

/** Gatekeeper: accepted, and accepted because it is notarized. */
export function evaluateSpctl(r) {
  return r.accepted && r.source === "Notarized Developer ID";
}

/** `xcrun stapler validate` -> stapled ticket present and valid. */
export function parseStapler(text, code) {
  return code === 0 && /The validate action worked!/.test(text);
}

/** `hdiutil imageinfo` -> {format, checksumType} ("Format: UDZO", "Checksum Type: CRC32"). */
export function parseImageInfo(text) {
  // RED STUB (WP-H-MACOS red commit): old behaviour, replaced in the green commit
  return { format: "", checksumType: "" };
}

/** `security find-identity -v -p codesigning` -> the Developer ID Application identities in it. */
export function developerIdIdentities(text) {
  // RED STUB (WP-H-MACOS red commit): old behaviour, replaced in the green commit
  return [];
}

/** `security dump-keychain` (attributes only, no -d) -> [{keychain, cls, service, account}] */
export function parseKeychainDump(text) {
  const items = [];
  let cur = null;
  const attr = (line, key) => {
    const m = new RegExp(`^\\s*"${key}"<blob>=(?:0x[0-9A-Fa-f]+\\s+)?"(.*)"\\s*$`).exec(line);
    return m ? m[1] : null;
  };
  for (const line of text.split("\n")) {
    const kc = /^keychain: "(.*)"$/.exec(line);
    if (kc) { cur = { keychain: kc[1], cls: "", service: "", account: "" }; items.push(cur); continue; }
    if (!cur) continue;
    const cls = /^class: (?:"(.*)"|(0x[0-9A-Fa-f]+))/.exec(line);
    if (cls) { cur.cls = cls[1] || cls[2]; continue; }
    const s = attr(line, "svce"); if (s !== null) cur.service = s;
    const a = attr(line, "acct"); if (a !== null) cur.account = a;
  }
  return items;
}

const itemKey = (i) => `${i.keychain}\u0000${i.cls}\u0000${i.service}\u0000${i.account}`;
export function diffKeychain(before, after) {
  const b = new Set(before.map(itemKey)), a = new Set(after.map(itemKey));
  return { added: after.filter((i) => !b.has(itemKey(i))), removed: before.filter((i) => !a.has(itemKey(i))) };
}

/** Items this run added: documented services stay after uninstall (as documented); any other
 *  service fails the check. */
export function evaluateKeychain(added, documented) {
  const services = new Set(documented.map((d) => d.service));
  const undocumented = added.filter((i) => !services.has(i.service));
  return { ok: undocumented.length === 0, documented: added.filter((i) => services.has(i.service)), undocumented };
}

/** `security list-keychains -d user` -> paths */
export function parseKeychainList(text) {
  return [...text.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

// ---------------------------------------------------------------- Mach-O digest (pure)
// A signed copy of a sidecar differs from its pinned bytes: Tauri re-signs every externalBin with
// the Developer ID, which rewrites the signature blob and the sizes that cover it. The digest below
// is the same for every signature of the same code: sha256 of the bytes before the signature, with
// the three size fields that depend on the signature set to zero (__LINKEDIT vmsize and filesize,
// LC_CODE_SIGNATURE datasize). A valid code signature (sign.sidecars) binds those bytes.
const MH_MAGIC_64 = 0xfeedfacf, FAT_MAGIC = 0xcafebabe, FAT_MAGIC_64 = 0xcafebabf;
const LC_SEGMENT_64 = 0x19, LC_CODE_SIGNATURE = 0x1d;

function thinDigest(buf) {
  if (buf.length < 32 || buf.readUInt32LE(0) !== MH_MAGIC_64) throw new Error("not a 64-bit little-endian Mach-O");
  const ncmds = buf.readUInt32LE(16), sizeofcmds = buf.readUInt32LE(20);
  if (32 + sizeofcmds > buf.length) throw new Error("load commands run past the end of the file");
  let off = 32, sigOff = -1;
  const zero = [];
  for (let i = 0; i < ncmds; i++) {
    if (off + 8 > 32 + sizeofcmds) throw new Error("truncated load command");
    const cmd = buf.readUInt32LE(off), size = buf.readUInt32LE(off + 4);
    if (size < 8 || off + size > 32 + sizeofcmds) throw new Error("bad load command size");
    if (cmd === LC_CODE_SIGNATURE) { sigOff = buf.readUInt32LE(off + 8); zero.push([off + 12, 4]); }
    if (cmd === LC_SEGMENT_64 && buf.toString("latin1", off + 8, off + 24).replace(/\0[\s\S]*$/, "") === "__LINKEDIT") {
      zero.push([off + 32, 8], [off + 48, 8]);
    }
    off += size;
  }
  if (sigOff < 0) throw new Error("no code signature (LC_CODE_SIGNATURE)");
  if (sigOff > buf.length || sigOff < 32 + sizeofcmds) throw new Error("code signature offset out of range");
  const b = Buffer.from(buf.subarray(0, sigOff));
  for (const [o, n] of zero) b.fill(0, o, o + n);
  return createHash("sha256").update(b).digest("hex");
}

/** Signature-independent digest of a thin or universal Mach-O file (see above). Throws on anything
 *  else, so a check built on it fails closed. */
export function machoCodeDigest(buf) {
  // RED STUB (WP-H-MACOS red commit): old behaviour, replaced in the green commit
  return createHash("sha256").update(buf).digest("hex");
}

// ---------------------------------------------------------------- bundle spec (pure)
/** Bundle layout and sidecar pins of a release: the externalBins of the release overlay and, per
 *  component in release/components.json, its pinned sha256 for aarch64-apple-darwin. */
export function releaseBundleSpec({ manifest, pinsText, overlay }) {
  // RED STUB (WP-H-MACOS red commit): old behaviour, replaced in the green commit
  return { release: "", bins: [], sidecars: [], errors: [] };
}

/** Contents/MacOS must hold exactly the main executable and the release's sidecars. */
export function evaluateBundleStructure({ macosEntries, exe, bins, hasCodeResources, hasResources, requireSignature }) {
  // RED STUB (WP-H-MACOS red commit): old behaviour, replaced in the green commit
  return { ok: true, why: [], missing: [], extra: [] };
}

/** Each bundled sidecar's code digest equals the code digest of its pinned bytes. `pinned` maps a
 *  pin sha256 to that file's code digest (from --pinned-dir, sha256-verified); `bundled` maps a
 *  sidecar name to its code digest, or to an Error when it could not be read. */
export function evaluateSidecarDigests(sidecars, pinned, bundled) {
  // RED STUB (WP-H-MACOS red commit): old behaviour, replaced in the green commit
  return { status: "pass", rows: [] };
}

// ---------------------------------------------------------------- data locations (pure)
/** data-locations.txt: `<path relative to HOME> | <why>` per line, `#` comments. */
export function parseLocations(text) {
  // RED STUB (WP-H-MACOS red commit): old behaviour, replaced in the green commit
  return [];
}

/** New files under `root` changed after `sinceMs`, depth-limited, skipping `exclude` subtrees and
 *  pruned names; never follows symlinks. Unreadable directories are skipped and counted. */
export function walkNewer(root, sinceMs, { maxDepth = 4, exclude = [], prune = [], cap = 400000 } = {}) {
  const found = [];
  let seen = 0, unreadable = 0, capped = false;
  const stack = [[root, 0]];
  while (stack.length) {
    const [dir, depth] = stack.pop();
    let entries;
    try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { unreadable++; continue; }
    for (const e of entries) {
      if (++seen > cap) { capped = true; break; }
      const p = path.join(dir, e.name);
      if (exclude.some((x) => p === x || isInside(p, x))) continue;
      let st;
      try { st = fs.lstatSync(p); } catch { continue; }
      if (Math.max(st.mtimeMs, st.ctimeMs, st.birthtimeMs || 0) > sinceMs) found.push(p);
      if (e.isDirectory() && !e.isSymbolicLink() && depth + 1 < maxDepth && !prune.includes(e.name)) stack.push([p, depth + 1]);
    }
    if (capped) break;
  }
  return { found, seen, unreadable, capped };
}

/** Does a path belong to Citrate: "citrate" anywhere, or a sidecar name or .ipfs as a segment. */
export function isCitratePath(p, exeNames) {
  const names = new Set([...exeNames, ".ipfs"]);
  return /citrate/i.test(p) || p.split("/").some((s) => names.has(s));
}

/** Split new paths: Citrate's paths under a documented location of one of `homes` are documented,
 *  Citrate's other paths are undocumented (the check fails on them), the rest is unrelated. A
 *  parent directory of a documented location (e.g. Library/Application Support itself) is
 *  unrelated, as it is not Citrate's. */
export function evaluateLocations(paths, homes, documented, exeNames) {
  // RED STUB (WP-H-MACOS red commit): old behaviour, replaced in the green commit
  return { ok: true, documented: [], undocumented: [], unrelated: [...paths] };
}

// ---------------------------------------------------------------- guards
/** Where the test-machine hosts file lives: under the account's real home. */
export function hostsFilePath(pwHome) {
  return path.join(pwHome, ".config", "citrate", "macos-acceptance-test-hosts");
}
export function normalizeHost(h) {
  return String(h || "").trim().toLowerCase().replace(/\.local$/, "");
}

/** The explicit test-machine marker, checked before any command runs. `hostsFile` is
 *  {uid, mode, text} from stat + read, or null when it does not exist. */
export function evaluateMarker({ platform, env, hostname, uid, hostsFile }) {
  // RED STUB (WP-H-MACOS red commit): old behaviour, replaced in the green commit
  return { ok: true, refusals: [] };
}

/** Paths whose existence means Citrate Core is installed or has run under this account. */
export function ownerPaths(pwHome) {
  return [
    `/Applications/${APP_DIR_NAME}`,
    path.join(pwHome, "Applications", APP_DIR_NAME),
    path.join(pwHome, "Library", "Application Support", BUNDLE_ID),
    path.join(pwHome, "Library", "Caches", BUNDLE_ID),
    path.join(pwHome, "Library", "WebKit", BUNDLE_ID),
    path.join(pwHome, "Library", "Preferences", `${BUNDLE_ID}.plist`),
    path.join(pwHome, ".citrate"),
    path.join(pwHome, ".citrate-wallet"),
  ];
}

/** Pure decision over gathered facts. Missing or failed probes refuse (fail closed). */
export function evaluateGuards(f) {
  const r = [];
  if (f.platform !== "darwin") r.push(`not macOS (${f.platform})`);
  if (f.uid === 0) r.push("running as root");
  for (const p of f.existingOwnerPaths ?? ["<probe missing>"]) r.push(`Citrate Core is installed or has data here: ${p}`);
  // Spotlight is advisory (indexing may be off); the path checks above are the hard guard.
  for (const p of f.spotlightApps || []) r.push(`a bundle with id ${BUNDLE_ID} exists: ${p}`);
  if (!f.procs) r.push("process probe did not run");
  else for (const p of f.procs.filter((q) => q.path.includes(`/${APP_DIR_NAME}/`))) r.push(`Citrate Core is running: pid ${p.pid} ${p.path}`);
  if (!f.listeners) r.push("listener probe did not run");
  else for (const l of f.listeners.filter((x) => NODE_PORTS.includes(splitHostPort(x.local).port))) {
    r.push(`port ${splitHostPort(l.local).port} is in use (pid ${l.pid} ${l.command}): a node runs on this machine`);
  }
  for (const s of KEYCHAIN_SERVICES) {
    const k = f.keychain?.[s];
    if (k === "absent") continue;
    r.push(k === "present" ? `the keychain holds a ${s} item` : `keychain probe for ${s} did not run cleanly`);
  }
  if (!f.signingIdentities) r.push("signing-identity probe did not run");
  else for (const id of f.signingIdentities) r.push(`a release signing identity is in the keychain (${id}): this is a release machine`);
  return { ok: r.length === 0, refusals: r };
}

/** Read-only probes. `pwHome` comes from the password database, so HOME=/elsewhere cannot hide
 *  the owner's data. */
export function gatherGuardFacts(deps) {
  const f = { platform: deps.platform, uid: deps.uid };
  if (deps.platform !== "darwin") return f;
  f.existingOwnerPaths = ownerPaths(deps.pwHome).filter((p) => deps.exists(p));
  const md = deps.read("mdfind", [`kMDItemCFBundleIdentifier == "${BUNDLE_ID}"`]);
  f.spotlightApps = md.code === 0 ? md.stdout.split("\n").filter(Boolean) : null;
  const ps = deps.read("ps", ["-axww", "-o", "pid=,ppid=,comm="]);
  f.procs = ps.code === 0 ? parsePs(ps.stdout) : null;
  const ls = deps.read("lsof", ["-nP", "-iTCP", "-sTCP:LISTEN", "-iUDP", "-F", "pcPn"]);
  f.listeners = ls.code === 0 || (ls.code === 1 && !ls.stdout.trim()) ? parseLsof(ls.stdout) : null;
  f.keychain = {};
  for (const s of KEYCHAIN_SERVICES) {
    const k = deps.read("security", ["find-generic-password", "-s", s]);
    f.keychain[s] = k.code === 0 ? "present" : k.code === 44 ? "absent" : "error";
  }
  const fi = deps.read("security", ["find-identity", "-v", "-p", "codesigning"]);
  f.signingIdentities = fi.code === 0 ? developerIdIdentities(fi.stdout) : null;
  return f;
}

// ---------------------------------------------------------------- command runner
// Read-only commands (by command + first argument) may run before the guards. Everything else
// is a mutation and throws until markGuarded() has been called.
const READ_ONLY = {
  ps: null, lsof: null, mdfind: null, sw_vers: null, getconf: null,
  plutil: ["-extract"], codesign: ["--verify", "-dvvv"], spctl: ["--assess"],
  security: ["find-generic-password", "find-identity", "dump-keychain", "list-keychains", "default-keychain"],
  xcrun: ["stapler"], syspolicy_check: ["distribution"], hdiutil: ["verify", "imageinfo"], git: ["rev-parse"],
};
export function isReadOnly(cmd, args) {
  const name = path.basename(cmd);
  if (!(name in READ_ONLY)) return false;
  const first = READ_ONLY[name];
  if (first === null) return true;
  if (!first.includes(args[0])) return false;
  // list-keychains / default-keychain only read when they carry no -s.
  if (name === "security" && args.includes("-s") && args[0] !== "find-generic-password") return false;
  if (name === "xcrun" && args[1] !== "validate") return false;
  return true;
}

export function createRunner(exec) {
  let guarded = false;
  return {
    markGuarded() { guarded = true; },
    get guarded() { return guarded; },
    read(cmd, args, o) {
      if (!isReadOnly(cmd, args)) throw new Error(`not a read-only command: ${cmd} ${args.join(" ")}`);
      return exec(cmd, args, o);
    },
    mut(cmd, args, o) {
      if (!guarded) throw new Error(`refusing to run ${cmd} before the machine-safety guards passed`);
      return exec(cmd, args, o);
    },
  };
}

// ---------------------------------------------------------------- verdict
export const REQUIRED = {
  inspect: ["dmg.sha256", "dmg.verify"],
  verify: ["dmg.sha256", "dmg.verify", "dmg.mount", "bundle.identity", "bundle.structure", "sidecar.digests",
    "sign.codesign", "sign.developer_id", "sign.sidecars", "gatekeeper.spctl", "notarization.staple"],
};
REQUIRED.full = [...REQUIRED.verify, "install.copy", "gatekeeper.quarantined", "launch.ready", "launch.rpc",
  "launch.listeners_loopback", "restart.sidecar", "quit.clean", "quit.listeners", "reap.orphans",
  "uninstall.processes", "data.locations", "uninstall.outside_files", "uninstall.keychain"];

/** overall from the checks. A run passes only if every required check of its mode ran and passed;
 *  an unsigned smoke run (the self-test's stand-in) can at best be pass-unsigned-smoke. */
export function computeOverall(checks, { unsignedSmoke, mode }) {
  if (checks.some((c) => c.status === "error")) return "error";
  if (checks.some((c) => c.status === "fail")) return "fail";
  const req = REQUIRED[mode];
  if (!req) return "error";
  const byId = new Map(checks.map((c) => [c.id, c.status]));
  const ran = req.every((id) => byId.get(id) === "pass" || (unsignedSmoke && byId.get(id) === "skip"));
  if (!ran) return "error";
  return unsignedSmoke ? "pass-unsigned-smoke" : "pass";
}

export function exitCodeFor(overall) {
  return overall === "pass" || overall === "pass-unsigned-smoke" ? 0 : overall === "fail" ? 1 : 2;
}

/** Only a release-mode full run that passed is evidence for G1 g1-macos. */
export function isG1Evidence(v) {
  // RED STUB (WP-H-MACOS red commit): old behaviour, replaced in the green commit
  return v.overall === "pass";
}

// ---------------------------------------------------------------- the run (macOS only)
const now = () => Date.now();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const envInt = (env, k, d) => { const n = Number(env[k]); return Number.isFinite(n) && n > 0 ? n : d; };

function snapshotProcs(rn) {
  const r = rn.read("ps", ["-axww", "-o", "pid=,ppid=,comm="]);
  if (r.code !== 0) throw new Error(`ps failed: ${r.stderr}`);
  return parsePs(r.stdout);
}
function snapshotListeners(rn) {
  const r = rn.read("lsof", ["-nP", "-iTCP", "-sTCP:LISTEN", "-iUDP", "-F", "pcPn"]);
  if (r.code !== 0 && r.stdout.trim()) throw new Error(`lsof failed: ${r.stderr}`);
  return parseLsof(r.stdout);
}

async function rpcChainId(url) {
  try {
    const res = await fetch(url, {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "eth_chainId", params: [] }),
      signal: AbortSignal.timeout(3000),
    });
    const j = await res.json();
    return typeof j.result === "string" ? parseInt(j.result, 16) : null;
  } catch { return null; }
}

/** Code digests of the files in --pinned-dir, keyed by each file's sha256. */
function pinnedDigests(dir) {
  const m = new Map();
  for (const n of fs.readdirSync(dir)) {
    const p = path.join(dir, n);
    if (!fs.lstatSync(p).isFile()) continue;
    const buf = fs.readFileSync(p);
    const sha = createHash("sha256").update(buf).digest("hex");
    try { m.set(sha, machoCodeDigest(buf)); } catch (e) { m.set(sha, e); }
  }
  return m;
}

export async function runAcceptance(ctx, o) {
  const { rn, deps, log } = ctx;
  const checks = [];
  const add = (id, status, detail, evidence) => {
    checks.push({ id, status, detail, ...(evidence === undefined ? {} : { evidence }) });
    log(`${status.toUpperCase().padEnd(5)} ${id}: ${detail}`);
    return status === "pass" || status === "skip" || status === "info";
  };
  const finish = () => ({ checks, overall: computeOverall(checks, o) });
  const smokeSkip = (cid, what) => add(cid, "skip", `unsigned smoke (stand-in): ${what}`);

  // ================= inspect: the DMG file only (no mount)
  const sha = createHash("sha256").update(fs.readFileSync(o.dmg)).digest("hex");
  if (!add("dmg.sha256", sha === o.sha256 ? "pass" : "fail", sha === o.sha256 ? "matches" : `got ${sha}`)) return finish();
  const ver = rn.read("hdiutil", ["verify", o.dmg]);
  if (!add("dmg.verify", ver.code === 0 ? "pass" : "fail", ver.code === 0 ? "image checksum valid" : (ver.stderr || ver.stdout).trim())) return finish();
  const info = rn.read("hdiutil", ["imageinfo", o.dmg]);
  const ii = parseImageInfo(info.stdout);
  add("dmg.format", "info", `format ${ii.format || "unknown"}, checksum ${ii.checksumType || "unknown"}`);
  const dmgSig = rn.read("codesign", ["-dvvv", o.dmg]);
  const dmgSigP = parseCodesignDisplay(dmgSig.stderr + dmgSig.stdout);
  const dmgSt = rn.read("xcrun", ["stapler", "validate", o.dmg]);
  const dmgSpR = rn.read("spctl", ["--assess", "--type", "open", "--context", "context:primary-signature", "-vv", o.dmg]);
  const dmgSp = parseSpctl(dmgSpR.stderr + dmgSpR.stdout, dmgSpR.code);
  // Gatekeeper decides on the app inside (verify mode); the DMG's own signature is reported.
  add("dmg.signature", "info", `dmg signed=${dmgSig.code === 0 ? (dmgSigP.teamId || "ad hoc") : "no"}, staple=${parseStapler(dmgSt.stdout + dmgSt.stderr, dmgSt.code) ? "present" : "absent"}, spctl(open)=${dmgSp.accepted ? "accepted" : "rejected"} ${dmgSp.source}`);
  if (o.mode === "inspect") return finish();

  // ================= verify: mount read-only, inspect the bundle in place (no copy, no launch)
  const mnt = path.join(ctx.run, "mnt");
  fs.mkdirSync(mnt);
  const at = rn.mut("hdiutil", ["attach", "-nobrowse", "-readonly", "-noautoopen", "-noverify", "-mountpoint", mnt, o.dmg]);
  if (!add("dmg.mount", at.code === 0 ? "pass" : "error", at.code === 0 ? `read-only at ${mnt}` : at.stderr.trim())) return finish();
  ctx.mounted = mnt;

  const appsInDmg = fs.readdirSync(mnt).filter((n) => n.endsWith(".app"));
  const srcApp = path.join(mnt, APP_DIR_NAME);
  const plist = path.join(srcApp, "Contents", "Info.plist");
  const pl = (k) => { const r = rn.read("plutil", ["-extract", k, "raw", "-o", "-", plist]); return r.code === 0 ? r.stdout.trim() : ""; };
  const id = appsInDmg.length === 1 && appsInDmg[0] === APP_DIR_NAME ? pl("CFBundleIdentifier") : "";
  const version = id ? pl("CFBundleShortVersionString") : "";
  const exe = id ? pl("CFBundleExecutable") : "";
  const want = o.expectVersion || ctx.spec.release;
  const versionOk = !!want && (version === want || version.startsWith(`${want}-`));
  const idOk = id === BUNDLE_ID && !!exe && versionOk;
  if (!add("bundle.identity", idOk ? "pass" : "fail", idOk ? `${id} ${version}` : `apps=${JSON.stringify(appsInDmg)} id=${id} version=${version} (want ${want || "?"}) exe=${exe}`, { version })) return finish();
  const macosDir = (app) => path.join(app, "Contents", "MacOS");
  const bundledExes = fs.readdirSync(macosDir(srcApp));
  const bs = evaluateBundleStructure({
    macosEntries: bundledExes, exe, bins: ctx.spec.bins,
    hasCodeResources: fs.existsSync(path.join(srcApp, "Contents", "_CodeSignature", "CodeResources")),
    hasResources: fs.existsSync(path.join(srcApp, "Contents", "Resources")), requireSignature: !o.unsignedSmoke,
  });
  add("bundle.structure", bs.ok ? "pass" : "fail", bs.ok ? `${exe} + ${ctx.spec.bins.length} sidecars, as the release overlay lists` : bs.why.join("; "));

  if (ctx.spec.errors.length) add("sidecar.digests", "error", `pins unreadable: ${ctx.spec.errors.join("; ")}`);
  else if (!o.pinnedDir) add("sidecar.digests", "error", "--pinned-dir not given: the pinned sidecar bytes are needed to compare (docs/MACOS_ACCEPTANCE.md)");
  else {
    const pinned = pinnedDigests(o.pinnedDir);
    const bundled = new Map();
    for (const s of ctx.spec.sidecars) {
      const p = path.join(macosDir(srcApp), s.bin);
      if (!fs.existsSync(p)) continue;
      try { bundled.set(s.bin, machoCodeDigest(fs.readFileSync(p))); } catch (e) { bundled.set(s.bin, e); }
    }
    const d = evaluateSidecarDigests(ctx.spec.sidecars, pinned, bundled);
    const bad = d.rows.filter((r) => r.status !== "pass");
    add("sidecar.digests", d.status, bad.length === 0 ? `${d.rows.length} sidecars match their pins` : bad.map((r) => `${r.bin}: ${r.why}`).join("; "), { rows: d.rows });
  }

  const cs = rn.read("codesign", ["--verify", "--deep", "--strict", "--verbose=2", srcApp]);
  const disp = rn.read("codesign", ["-dvvv", srcApp]);
  const sig = parseCodesignDisplay(disp.stderr + disp.stdout);
  const devId = evaluateDeveloperId(sig, o.teamId);
  const sidecarBad = [];
  for (const b of bundledExes.filter((n) => n !== exe)) {
    const p = path.join(macosDir(srcApp), b);
    const v = rn.read("codesign", ["--verify", "--strict", p]);
    const d = parseCodesignDisplay((() => { const r = rn.read("codesign", ["-dvvv", p]); return r.stderr + r.stdout; })());
    if (v.code !== 0 || !d.teamId || d.teamId !== sig.teamId || !d.runtime) sidecarBad.push(`${b} (verify=${v.code} team=${d.teamId || "none"} runtime=${d.runtime})`);
  }
  const sp = rn.read("spctl", ["--assess", "--type", "execute", "-vv", srcApp]);
  const spr = parseSpctl(sp.stderr + sp.stdout, sp.code);
  const st = rn.read("xcrun", ["stapler", "validate", srcApp]);
  const stapled = parseStapler(st.stdout + st.stderr, st.code);
  const sy = deps.exists("/usr/bin/syspolicy_check") ? rn.read("syspolicy_check", ["distribution", srcApp]) : null;
  if (o.unsignedSmoke) {
    smokeSkip("sign.codesign", `codesign verify exit ${cs.code}`);
    smokeSkip("sign.developer_id", devId.why.join("; ") || "ok");
    smokeSkip("sign.sidecars", `${sidecarBad.length} not release-signed`);
    smokeSkip("gatekeeper.spctl", `${spr.accepted ? "accepted" : "rejected"} ${spr.source}`);
    smokeSkip("notarization.staple", stapled ? "present" : "absent");
  } else {
    add("sign.codesign", cs.code === 0 ? "pass" : "fail", cs.code === 0 ? "deep strict verify ok" : cs.stderr.trim());
    add("sign.developer_id", devId.ok ? "pass" : "fail", devId.ok ? `team ${sig.teamId}, hardened runtime` : devId.why.join("; "), { teamId: sig.teamId, authorities: sig.authorities });
    add("sign.sidecars", sidecarBad.length === 0 ? "pass" : "fail", sidecarBad.length === 0 ? `${bundledExes.length - 1} sidecars signed by team ${sig.teamId}, hardened runtime` : sidecarBad.join(", "));
    add("gatekeeper.spctl", evaluateSpctl(spr) ? "pass" : "fail", `${spr.accepted ? "accepted" : "rejected"} source=${spr.source || "none"}`);
    add("notarization.staple", stapled ? "pass" : "fail", stapled ? "ticket stapled to the app" : (st.stdout + st.stderr).trim());
  }
  if (sy) add("gatekeeper.syspolicy", sy.code === 0 || o.unsignedSmoke ? "info" : "fail", (sy.stdout + sy.stderr).trim().split("\n").slice(-3).join(" | "));
  if (o.mode === "verify") return finish();

  // ================= full: install into the run dir, launch, quit, reap, uninstall
  const apps = path.join(ctx.run, "Applications");
  const installApp = path.join(apps, APP_DIR_NAME);
  const home = path.join(ctx.run, "home");
  const tmp = path.join(ctx.run, "tmp");
  const ownPids = new Map(); // pid -> executable path when first seen
  let launches = 0;
  const T = {
    launch: envInt(deps.env, "MACOS_ACC_LAUNCH_TIMEOUT", 240) * 1000,
    restart: envInt(deps.env, "MACOS_ACC_RESTART_TIMEOUT", 120) * 1000,
    quit: envInt(deps.env, "MACOS_ACC_QUIT_TIMEOUT", 60) * 1000,
    settle: envInt(deps.env, "MACOS_ACC_SETTLE_SECS", 15) * 1000,
  };
  fs.mkdirSync(apps);
  const cp = rn.mut("ditto", [srcApp, installApp]);
  if (!add("install.copy", cp.code === 0 ? "pass" : "error", cp.code === 0 ? installApp : cp.stderr.trim())) return finish();
  ctx.installApp = installApp;

  // Gatekeeper on the installed copy marked as downloaded by a browser (quarantine), which is what
  // the first-launch check sees. The dialog itself is the manual step in the runbook.
  const q = `0081;${Math.floor(now() / 1000).toString(16)};Safari;`;
  rn.mut("xattr", ["-w", "com.apple.quarantine", q, installApp]);
  const sq = rn.read("spctl", ["--assess", "--type", "execute", "-vv", installApp]);
  const sqr = parseSpctl(sq.stderr + sq.stdout, sq.code);
  if (o.unsignedSmoke) smokeSkip("gatekeeper.quarantined", `${sqr.accepted ? "accepted" : "rejected"} ${sqr.source}`);
  else add("gatekeeper.quarantined", evaluateSpctl(sqr) ? "pass" : "fail", `quarantined copy ${sqr.accepted ? "accepted" : "rejected"} source=${sqr.source || "none"}`);
  rn.mut("xattr", ["-dr", "com.apple.quarantine", installApp]);

  const appExe = path.join(macosDir(installApp), exe);
  const wildAllow = parseAllowlist(fs.readFileSync(path.join(DATA_DIR, "wildcard-allowlist.txt"), "utf8"), ["exe", "proto", "port", "reason"]);
  fs.mkdirSync(home, { recursive: true });
  fs.mkdirSync(tmp, { recursive: true });
  ctx.home = home;
  const ours = (procs, appPid) => {
    const d = appPid ? descendants(procs, appPid) : new Set();
    for (const p of procs) if (d.has(p.pid) && !ownPids.has(p.pid)) ownPids.set(p.pid, p.path);
    return procs.filter((p) => p.pid === appPid || d.has(p.pid) || isInside(p.path, installApp));
  };
  const launch = () => {
    launches++;
    const env = {
      PATH: "/usr/bin:/bin:/usr/sbin:/sbin", HOME: home, CFFIXED_USER_HOME: home, TMPDIR: tmp + "/",
      USER: deps.env.USER || "", LOGNAME: deps.env.LOGNAME || deps.env.USER || "", LANG: "en_US.UTF-8", SHELL: "/bin/zsh",
    };
    const pid = deps.spawnApp(rn, appExe, env, path.join(ctx.logs, `app-launch${launches}.log`));
    const self = snapshotProcs(rn).find((p) => p.pid === pid);
    ownPids.set(pid, self ? self.path : appExe);
    return pid;
  };
  const sidecarsOf = (procs, appPid) => ours(procs, appPid).filter((p) => p.pid !== appPid && isInside(p.path, macosDir(installApp)));
  const waitFor = async (ms, fn) => { const end = now() + ms; for (;;) { const v = await fn(); if (v) return v; if (now() > end) return null; await sleep(1000); } };
  const killOwn = (pids, sig) => {
    const procs = snapshotProcs(rn);
    for (const pid of pids) {
      const p = procs.find((x) => x.pid === pid);
      if (p && mayKill(p, installApp, ownPids)) { try { deps.kill(pid, sig); } catch { /* gone */ } }
    }
  };
  ctx.killOwn = killOwn;
  ctx.ownPids = ownPids;
  const quit = (pid) => {
    if (o.quitMethod === "term") { killOwn([pid], "SIGTERM"); return { ok: true, how: "SIGTERM (stand-in only)" }; }
    // NSRunningApplication.terminate: the same quit request as Cmd-Q, addressed by pid.
    const js = `ObjC.import("AppKit"); var a = $.NSRunningApplication.runningApplicationWithProcessIdentifier(${Number(pid)}); a.isNil() ? "none" : (a.terminate() ? "sent" : "refused")`;
    const r = rn.mut("osascript", ["-l", "JavaScript", "-e", js]);
    return { ok: r.code === 0 && r.stdout.trim() === "sent", how: `NSRunningApplication.terminate -> ${r.stdout.trim() || r.stderr.trim()}` };
  };
  const ready = async (pid) => waitFor(T.launch, () => {
    const procs = snapshotProcs(rn);
    if (!procs.some((p) => p.pid === pid)) return { dead: true };
    const names = new Set(sidecarsOf(procs, pid).map((p) => path.basename(p.path)));
    return o.required.every((r) => names.has(r)) ? { names: [...names] } : null;
  });

  // --- first launch: required sidecars up, still alive after the settle time, RPC answers 40204
  const baseline = snapshotListeners(rn);
  const baseKeys = new Set(baseline.map((l) => `${l.proto} ${l.local}`));
  let appPid = launch();
  const r1 = await ready(appPid);
  if (r1 && !r1.dead) await sleep(T.settle);
  const alive = r1 && !r1.dead && snapshotProcs(rn).some((p) => p.pid === appPid);
  if (!add("launch.ready", alive ? "pass" : "fail", alive ? `required sidecars up (${o.required.join(", ")}) and the app alive after ${T.settle / 1000}s` : r1?.dead || r1 ? "the app exited during launch" : `required sidecars not up within ${T.launch / 1000}s`)) return finish();
  if (o.rpc) {
    const cid = await waitFor(T.launch, () => rpcChainId("http://127.0.0.1:8545"));
    add("launch.rpc", cid === EXPECTED_CHAIN_ID ? "pass" : "fail", cid === null ? "no eth_chainId answer on 127.0.0.1:8545" : `eth_chainId ${cid}`);
  } else smokeSkip("launch.rpc", "no node RPC in the stand-in");
  let procs = snapshotProcs(rn);
  const mine = ours(procs, appPid);
  const pathByPid = new Map(mine.map((p) => [p.pid, p.path]));
  const myListeners = snapshotListeners(rn).filter((l) => pathByPid.has(l.pid));
  const appPorts = new Set(myListeners.map((l) => `${l.proto} ${l.local}`));
  const lv = evaluateListeners(myListeners, pathByPid, wildAllow);
  add("launch.listeners_loopback", lv.ok ? "pass" : "fail",
    lv.ok ? `${lv.loopback.length} loopback, ${lv.allowed.length} documented wildcard` : `not loopback: ${lv.bad.map((b) => `${b.exe} ${b.proto} ${b.local}`).join(", ")}`,
    { sidecars: sidecarsOf(procs, appPid).map((p) => path.basename(p.path)), listeners: lv });

  // --- restart: SIGKILL the node sidecar; the supervisor must start a new one under the app
  const target = sidecarsOf(procs, appPid).find((p) => path.basename(p.path) === o.restartTarget);
  if (!target) add("restart.sidecar", "fail", `${o.restartTarget} not running`);
  else {
    const before = new Set(sidecarsOf(procs, appPid).filter((p) => path.basename(p.path) === o.restartTarget).map((p) => p.pid));
    killOwn([target.pid], "SIGKILL");
    const back = await waitFor(T.restart, () => sidecarsOf(snapshotProcs(rn), appPid).find((p) => path.basename(p.path) === o.restartTarget && !before.has(p.pid)));
    add("restart.sidecar", back ? "pass" : "fail", back ? `pid ${target.pid} -> ${back.pid}` : `not restarted within ${T.restart / 1000}s`);
  }

  // --- quit: Cmd-Q equivalent; the app and everything it started are gone, no listener left
  const quitAndCheck = async (pid, idClean, idListeners) => {
    const before = ours(snapshotProcs(rn), pid).map((p) => p.pid);
    const q1 = quit(pid);
    if (!q1.ok) { add(idClean, "error", `quit not delivered: ${q1.how}`); return false; }
    const gone = await waitFor(T.quit, () => {
      const ps1 = snapshotProcs(rn);
      return ps1.filter((p) => before.includes(p.pid) || isInside(p.path, installApp)).length === 0;
    });
    const left = snapshotProcs(rn).filter((p) => before.includes(p.pid) || isInside(p.path, installApp));
    add(idClean, gone ? "pass" : "fail", gone ? `${q1.how}; ${before.length} processes ended` : `left after ${T.quit / 1000}s: ${left.map((p) => `${p.pid} ${path.basename(p.path)}`).join(", ")}`);
    const leftL = snapshotListeners(rn).filter((l) => before.includes(l.pid) || (appPorts.has(`${l.proto} ${l.local}`) && !baseKeys.has(`${l.proto} ${l.local}`)));
    if (idListeners) add(idListeners, leftL.length === 0 ? "pass" : "fail", leftL.length === 0 ? "no listener of the app left" : leftL.map((l) => `${l.pid} ${l.command} ${l.proto} ${l.local}`).join(", "));
    if (left.length) killOwn(left.map((p) => p.pid), "SIGKILL"); // so later checks stand alone
    return gone;
  };
  await quitAndCheck(appPid, "quit.clean", "quit.listeners");

  // --- reap: SIGKILL the app only; the next launch must stop the orphaned sidecars it left
  appPid = launch();
  const r2 = await ready(appPid);
  if (!r2 || r2.dead) add("reap.orphans", "fail", "second launch did not reach ready");
  else {
    const kids = [...descendants(snapshotProcs(rn), appPid)];
    killOwn([appPid], "SIGKILL");
    await sleep(2000);
    const survivors = snapshotProcs(rn).filter((p) => kids.includes(p.pid)).map((p) => p.pid);
    appPid = launch();
    const reaped = await waitFor(T.quit, () => snapshotProcs(rn).filter((p) => survivors.includes(p.pid)).length === 0);
    const r3 = await ready(appPid);
    const ok = !!reaped && !!r3 && !r3.dead;
    add("reap.orphans", ok ? "pass" : "fail", ok ? `${survivors.length} orphans of the killed app stopped by the next launch` : !reaped ? `orphans still running: ${survivors.join(", ")}` : "relaunch after the kill did not reach ready", { survivors });
    const leftover = snapshotProcs(rn).filter((p) => survivors.includes(p.pid)).map((p) => p.pid);
    if (leftover.length) killOwn(leftover, "SIGKILL");
    await quitAndCheck(appPid, "reap.quit", null);
  }

  // --- uninstall: drag to Trash = remove the bundle; then nothing of it may run
  procs = snapshotProcs(rn);
  const stray = procs.filter((p) => mayKill(p, installApp, ownPids));
  rn.mut("rm", ["-rf", "--", installApp]);
  add("uninstall.processes", stray.length === 0 ? "pass" : "fail", stray.length === 0 ? "no process of the bundle running" : stray.map((p) => `${p.pid} ${p.path}`).join(", "));
  if (stray.length) killOwn(stray.map((p) => p.pid), "SIGKILL");
  return { ...finish(), exeNames: bundledExes };
}

/** After uninstall: what the app left in its isolated HOME, outside the run dir, and in the
 *  keychain. Each must be documented. */
export function finalScans(ctx, o, exeNames) {
  const { deps, rn } = ctx;
  const checks = [];
  const documented = parseLocations(fs.readFileSync(path.join(DATA_DIR, "data-locations.txt"), "utf8"));
  const realHome = (() => { try { return fs.realpathSync(deps.pwHome); } catch { return deps.pwHome; } })();
  const isoHome = fs.realpathSync(ctx.home);

  // 1. inside the isolated HOME: everything Citrate wrote is under a documented location.
  const keychainDir = path.join(isoHome, "Library", "Keychains"); // the harness's throwaway keychain
  const inHome = walkNewer(isoHome, 0, { maxDepth: 6, exclude: [keychainDir] });
  const lh = evaluateLocations(inHome.found, [isoHome], documented, exeNames);
  const docTop = documented.filter((d) => fs.existsSync(path.join(isoHome, d.prefix))).map((d) => d.prefix);
  checks.push({ id: "data.locations", status: lh.ok ? "pass" : "fail",
    detail: lh.ok ? `app data only in documented locations (${docTop.join(", ") || "none written"})` : `undocumented: ${lh.undocumented.slice(0, 20).map((p) => path.relative(isoHome, p)).join(", ")}`,
    evidence: { present: docTop, undocumented: lh.undocumented.slice(0, 200), other: lh.unrelated.slice(0, 50).map((p) => path.relative(isoHome, p)) } });

  // 2. outside the run dir: the real home, shared temp dirs.
  const roots = [{ root: realHome, maxDepth: 5 }, { root: "/private/tmp", maxDepth: 3 }, { root: "/Users/Shared", maxDepth: 3 }];
  for (const k of ["DARWIN_USER_TEMP_DIR", "DARWIN_USER_CACHE_DIR"]) {
    const r = rn.read("getconf", [k]);
    if (r.code === 0 && r.stdout.trim()) { try { roots.push({ root: fs.realpathSync(r.stdout.trim()), maxDepth: 3 }); } catch { /* absent */ } }
  }
  const exclude = [ctx.work, REPO_ROOT, ctx.logDirAbs, path.dirname(path.resolve(o.dmg)), o.pinnedDir && path.resolve(o.pinnedDir)]
    .filter(Boolean).map((p) => { try { return fs.realpathSync(p); } catch { return p; } });
  // Large tool caches, and the folders macOS guards with a privacy prompt (the app does not write
  // there; reading them could block on a consent dialog).
  const prune = ["hostedtoolcache", ".cargo", ".rustup", "node_modules", ".npm", ".git", "Developer", "Homebrew",
    "Desktop", "Documents", "Downloads", "Pictures", "Movies", "Music", "Mail", "Messages", "Mobile Documents", "CloudStorage"];
  let found = [];
  const meta = [];
  for (const r of roots) {
    const w = walkNewer(r.root, ctx.startedMs, { maxDepth: r.maxDepth, exclude, prune });
    found = found.concat(w.found);
    meta.push({ root: r.root, seen: w.seen, unreadable: w.unreadable, capped: w.capped });
  }
  // A documented location under the real home is allowed too (cfprefsd writes preferences there
  // on the app's behalf); anything else of Citrate's outside the run dir fails.
  const lo = evaluateLocations(found, [realHome], documented, exeNames);
  checks.push({ id: "uninstall.outside_files", status: lo.ok ? "pass" : "fail",
    detail: lo.ok ? `no undocumented Citrate file outside the run dir (${lo.documented.length} documented, ${lo.unrelated.length} unrelated changes not counted)` : `${lo.undocumented.length} undocumented Citrate files outside the run dir: ${lo.undocumented.slice(0, 10).join(", ")}`,
    evidence: { undocumented: lo.undocumented.slice(0, 200), documented: lo.documented.slice(0, 50), unrelated_sample: lo.unrelated.slice(0, 50), roots: meta } });

  // 3. keychain items the run added: only documented services.
  const after = ctx.keychains.flatMap((k) => parseKeychainDump(rn.read("security", ["dump-keychain", k]).stdout));
  const d = diffKeychain(ctx.keychainBefore, after);
  const doc = parseAllowlist(fs.readFileSync(path.join(DATA_DIR, "keychain-allowlist.txt"), "utf8"), ["service", "reason"]);
  const ev = evaluateKeychain(d.added, doc);
  checks.push({ id: "uninstall.keychain", status: ev.ok ? "pass" : "fail",
    detail: ev.ok ? `${ev.documented.length} documented items remain (${[...new Set(ev.documented.map((i) => i.service))].join(", ") || "none"})` : `undocumented items: ${ev.undocumented.map((i) => `${i.service}/${i.account}`).join(", ")}`,
    evidence: { documented: ev.documented, undocumented: ev.undocumented, removed: d.removed.length } });
  return checks;
}

// ---------------------------------------------------------------- throwaway keychain (full only)
function setUpKeychain(ctx) {
  const { rn } = ctx;
  const listR = rn.read("security", ["list-keychains", "-d", "user"]);
  const defR = rn.read("security", ["default-keychain", "-d", "user"]);
  if (listR.code !== 0 || defR.code !== 0) throw new Error("cannot read the keychain search list");
  ctx.kcRestore = { list: parseKeychainList(listR.stdout), def: parseKeychainList(defR.stdout)[0] || "" };
  ctx.log(`keychain restore point: default=${ctx.kcRestore.def} list=${ctx.kcRestore.list.join(" ")}`);
  const kc = path.join(ctx.run, "home", "Library", "Keychains", "login.keychain-db");
  fs.mkdirSync(path.dirname(kc), { recursive: true });
  const pw = randomBytes(16).toString("hex");
  for (const args of [["create-keychain", "-p", pw, kc], ["set-keychain-settings", kc], ["unlock-keychain", "-p", pw, kc]]) {
    const r = rn.mut("security", args);
    if (r.code !== 0) throw new Error(`security ${args[0]} failed: ${r.stderr}`);
  }
  ctx.kcSet = true;
  rn.mut("security", ["list-keychains", "-d", "user", "-s", kc, ...ctx.kcRestore.list]);
  rn.mut("security", ["default-keychain", "-d", "user", "-s", kc]);
  ctx.keychains = [kc, ...ctx.kcRestore.list.filter((k) => fs.existsSync(k))];
  ctx.keychainBefore = ctx.keychains.flatMap((k) => parseKeychainDump(rn.read("security", ["dump-keychain", k]).stdout));
}
function restoreKeychain(ctx) {
  if (!ctx.kcSet || !ctx.kcRestore) return;
  const { rn } = ctx;
  if (ctx.kcRestore.def) rn.mut("security", ["default-keychain", "-d", "user", "-s", ctx.kcRestore.def]);
  rn.mut("security", ["list-keychains", "-d", "user", "-s", ...ctx.kcRestore.list]);
  ctx.kcSet = false;
}

// ---------------------------------------------------------------- self-test (stand-in app)
// A stand-in Citrate Core.app (bash main executable; its two sidecars are one small C program
// compiled here, so they are thin arm64/x86_64 binaries an ad hoc signature can run) packed into a
// real DMG and run in a good mode and broken modes. The good one must give pass-unsigned-smoke;
// each broken one must fail exactly its own check. Release-mode runs of the ad hoc stand-in must
// fail the signing checks (negative control).
export const SELF_TEST_MODES = [
  { mode: "good", run: "full", expectFail: [] },
  { mode: "leak", run: "full", expectFail: ["quit.clean"] },
  { mode: "wildcard", run: "full", expectFail: ["launch.listeners_loopback"] },
  { mode: "norestart", run: "full", expectFail: ["restart.sidecar"] },
  { mode: "noreap", run: "full", expectFail: ["reap.orphans"] },
  { mode: "stray-file", run: "full", expectFail: ["uninstall.outside_files"] },
  { mode: "undocumented-data", run: "full", expectFail: ["data.locations"] },
  { mode: "keychain", run: "full", expectFail: ["uninstall.keychain"] },
  { mode: "tampered-sidecar", run: "verify", expectFail: ["sidecar.digests"] },
  { mode: "extra-binary", run: "verify", expectFail: ["bundle.structure"] },
];

/** Does a self-test run match its mode? */
export function judgeSelfTest(expectFail, verdict) {
  const failed = verdict.checks.filter((c) => c.status === "fail" || c.status === "error").map((c) => c.id).sort();
  const want = [...expectFail].sort();
  const ok = JSON.stringify(failed) === JSON.stringify(want) && (want.length ? verdict.overall === "fail" : verdict.overall === "pass-unsigned-smoke");
  return { ok, failed };
}

export const STAND_IN_SIDECAR_C = `/* Stand-in sidecar for the macOS acceptance self-test. Not Citrate. */
#include <arpa/inet.h>
#include <netinet/in.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>
int main(int argc, char **argv) {
  if (argc == 4 && strcmp(argv[1], "listen") == 0) {
    int s = socket(AF_INET, SOCK_STREAM, 0), one = 1;
    struct sockaddr_in a;
    setsockopt(s, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_port = htons((unsigned short)atoi(argv[3]));
    if (inet_pton(AF_INET, argv[2], &a.sin_addr) != 1) return 2;
    if (bind(s, (struct sockaddr *)&a, sizeof a) != 0 || listen(s, 8) != 0) return 3;
    for (;;) { int c = accept(s, 0, 0); if (c >= 0) close(c); }
  }
  if (argc == 2) { sleep((unsigned)atoi(argv[1])); return 0; }
  return 64;
}
`;

export const STAND_IN_APP = `#!/bin/bash
# Stand-in Citrate Core for the macOS acceptance self-test. Not the app.
D="$(cd "$(dirname "$0")" && pwd)"
MODE="$(cat "$D/../Resources/selftest-mode")"
PORT="$(cat "$D/../Resources/selftest-port")"
PF="$HOME/.selftest-pids"
if [ "$MODE" != noreap ] && [ -f "$PF" ]; then
  while read -r p; do kill -KILL "$p" 2>/dev/null; done < "$PF"
fi
: > "$PF"
node_loop() {
  while :; do
    "$D/citrate" listen 127.0.0.1 "$PORT" & echo $! >> "$PF"; wait $!
    [ "$MODE" = norestart ] && break
    sleep 1
  done
}
node_loop & echo $! >> "$PF"
"$D/ipfs" 100000 & IPFS=$!; echo $IPFS >> "$PF"
if [ "$MODE" = wildcard ]; then "$D/citrate" listen 0.0.0.0 $((PORT + 1)) & echo $! >> "$PF"; fi
if [ "$MODE" = stray-file ]; then : > "/private/tmp/citrate-selftest-stray-$PORT"; fi
if [ "$MODE" = undocumented-data ]; then mkdir -p "$HOME/Library/citrate-undocumented" && : > "$HOME/Library/citrate-undocumented/x"; fi
mkdir -p "$HOME/Library/Application Support/ai.citrate.core" && : > "$HOME/Library/Application Support/ai.citrate.core/selftest"
if [ "$MODE" = keychain ]; then /usr/bin/security add-generic-password -s ai.citrate.selftest-undocumented -a selftest -w x; fi
quit() {
  while read -r p; do
    if [ "$MODE" = leak ] && [ "$p" = "$IPFS" ]; then continue; fi
    kill -TERM "$p" 2>/dev/null
  done < "$PF"
  rm -f "$PF"; exit 0
}
trap quit TERM
while :; do sleep 1 & wait $!; done
`;

/** Compile the stand-in sidecar twice: the pinned build and a differently optimised one (other
 *  bytes, same behaviour) for the tampered mode. */
function buildStandInSidecars(ctx) {
  const { rn } = ctx;
  const dir = fs.mkdtempSync(path.join(ctx.work, "stand-in-src-"));
  const src = path.join(dir, "sidecar.c");
  fs.writeFileSync(src, STAND_IN_SIDECAR_C);
  const pinnedDir = path.join(dir, "pinned");
  fs.mkdirSync(pinnedDir);
  const good = path.join(pinnedDir, "sidecar");
  const other = path.join(dir, "sidecar-O0");
  for (const [opt, out] of [["-O2", good], ["-O0", other]]) {
    const r = rn.mut("xcrun", ["clang", opt, "-o", out, src]);
    if (r.code !== 0) throw new Error(`the self-test needs a C compiler (xcode-select --install): ${r.stderr}`);
  }
  const sha = createHash("sha256").update(fs.readFileSync(good)).digest("hex");
  return { dir, pinnedDir, good, other, sha };
}

function buildStandInDmg(ctx, mode, port, sc) {
  const { rn } = ctx;
  const stage = fs.mkdtempSync(path.join(ctx.work, `stand-in-${mode}-`));
  const app = path.join(stage, APP_DIR_NAME);
  const mac = path.join(app, "Contents", "MacOS");
  const res = path.join(app, "Contents", "Resources");
  fs.mkdirSync(mac, { recursive: true });
  fs.mkdirSync(res, { recursive: true });
  fs.writeFileSync(path.join(app, "Contents", "Info.plist"), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>${BUNDLE_ID}</string>
<key>CFBundleExecutable</key><string>Citrate Core</string>
<key>CFBundleShortVersionString</key><string>0.0.0-selftest</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
`);
  fs.writeFileSync(path.join(mac, "Citrate Core"), STAND_IN_APP, { mode: 0o755 });
  fs.copyFileSync(sc.good, path.join(mac, "citrate"));
  fs.copyFileSync(mode === "tampered-sidecar" ? sc.other : sc.good, path.join(mac, "ipfs"));
  if (mode === "extra-binary") fs.copyFileSync(sc.good, path.join(mac, "unlisted-helper"));
  for (const n of fs.readdirSync(mac)) fs.chmodSync(path.join(mac, n), 0o755);
  fs.writeFileSync(path.join(res, "selftest-mode"), mode);
  fs.writeFileSync(path.join(res, "selftest-port"), String(port));
  const cs = rn.mut("codesign", ["--force", "--deep", "-s", "-", app]);
  if (cs.code !== 0) throw new Error(`codesign (ad hoc) of the stand-in failed: ${cs.stderr}`);
  const dmg = path.join(ctx.work, `stand-in-${mode}.dmg`);
  const r = rn.mut("hdiutil", ["create", "-quiet", "-fs", "HFS+", "-srcfolder", stage, "-volname", "Citrate Core", "-format", "UDZO", dmg]);
  if (r.code !== 0) throw new Error(`hdiutil create failed: ${r.stderr}`);
  rn.mut("rm", ["-rf", "--", stage]);
  return { dmg, sha256: createHash("sha256").update(fs.readFileSync(dmg)).digest("hex") };
}

// ---------------------------------------------------------------- orchestration
async function oneRun(base, o, label, spec) {
  const { deps, rn } = base;
  const run = fs.mkdtempSync(path.join(base.work, "run."));
  const logs = path.join(run, "logs");
  fs.mkdirSync(logs);
  const lines = [];
  const log = (s) => { lines.push(s); deps.out(`[${label}] ${s}`); };
  const ctx = { ...base, run, logs, log, spec, startedMs: now() - 1000, keychains: [], keychainBefore: [] };
  const cleanupOwn = () => { if (ctx.killOwn && ctx.ownPids) { try { ctx.killOwn([...ctx.ownPids.keys()], "SIGKILL"); } catch { /* best effort */ } } };
  const started = new Date().toISOString();
  let res = { checks: [], overall: "error" };
  const restoreOnSignal = () => {
    try { cleanupOwn(); restoreKeychain(ctx); if (ctx.mounted) rn.mut("hdiutil", ["detach", "-force", ctx.mounted]); } finally { process.exit(2); }
  };
  process.once("SIGINT", restoreOnSignal);
  process.once("SIGTERM", restoreOnSignal);
  try {
    if (o.mode === "full") setUpKeychain(ctx);
    res = await runAcceptance(ctx, o);
    if (o.mode === "full" && res.checks.some((c) => c.id === "uninstall.processes")) {
      res.checks.push(...finalScans(ctx, o, res.exeNames || []));
      res.overall = computeOverall(res.checks, o);
    }
  } catch (e) {
    res.checks.push({ id: "harness", status: "error", detail: String((e && e.stack) || e) });
    res.overall = "error";
  } finally {
    cleanupOwn();
    try { restoreKeychain(ctx); } catch (e) { log(`KEYCHAIN RESTORE FAILED: ${e}. Restore by hand from the restore point above.`); }
    if (ctx.mounted) rn.mut("hdiutil", ["detach", "-force", ctx.mounted]);
    process.removeListener("SIGINT", restoreOnSignal);
    process.removeListener("SIGTERM", restoreOnSignal);
  }
  const verdict = {
    schema: SCHEMA, label, started_at: started, finished_at: new Date().toISOString(),
    host: base.host, repo_rev: base.repoRev, mode: o.mode, unsigned_smoke: !!o.unsignedSmoke,
    dmg: { path: o.dmg, sha256: o.sha256 }, quit_method: o.quitMethod, overall: res.overall, checks: res.checks,
  };
  verdict.g1_evidence = isG1Evidence(verdict);
  fs.writeFileSync(path.join(logs, "verdict.json"), JSON.stringify(verdict, null, 2) + "\n");
  fs.writeFileSync(path.join(logs, "summary.txt"), lines.join("\n") + "\n");
  const dst = path.join(base.logDirAbs, label);
  fs.mkdirSync(dst, { recursive: true });
  fs.cpSync(logs, dst, { recursive: true });
  // Remove only this run's own directory (a run.* child of the work dir, never a symlink).
  if (isInside(run, base.work) && path.basename(run).startsWith("run.") && !fs.lstatSync(run).isSymbolicLink() && deps.env.MACOS_ACC_KEEP !== "1") {
    rn.mut("rm", ["-rf", "--", run]);
  }
  return verdict;
}

/** The release's bundle spec from this checkout (run the harness from the RC's tag). */
export function loadReleaseSpec(root = REPO_ROOT) {
  return releaseBundleSpec({
    manifest: loadManifest(path.join(root, "release", "components.json")),
    pinsText: fs.readFileSync(path.join(root, "src-tauri", "runtime-deps.sha256"), "utf8"),
    overlay: JSON.parse(fs.readFileSync(path.join(root, RELEASE_OVERLAY), "utf8")),
  });
}

export async function main(argv, deps) {
  let o;
  try { o = parseArgs(argv); } catch (e) {
    if (e instanceof UsageError) { deps.err(`usage: ${e.message}`); return 2; }
    throw e;
  }
  const refuse = (refusals) => {
    deps.err("REFUSED: this machine is not a test machine for the Citrate Core macOS acceptance:");
    for (const r of refusals) deps.err(`  - ${r}`);
    deps.out(JSON.stringify({ schema: SCHEMA, overall: "refused", refusals }));
    return 2;
  };
  // 1. The explicit marker, before any command runs.
  const mk = evaluateMarker({ platform: deps.platform, env: deps.env, hostname: deps.hostname, uid: deps.uid, hostsFile: deps.readHostsFile(hostsFilePath(deps.pwHome)) });
  if (!mk.ok) return refuse(mk.refusals);
  // 2. Read-only probes of the machine.
  const rn = createRunner(deps.exec);
  const g = evaluateGuards(gatherGuardFacts({ ...deps, read: rn.read }));
  if (!g.ok) return refuse(g.refusals);
  rn.markGuarded();

  const sw = rn.read("sw_vers", []);
  const rev = rn.read("git", ["rev-parse", "HEAD"], { cwd: REPO_ROOT });
  const base = {
    deps, rn, repoRev: rev.code === 0 ? rev.stdout.trim() : "unknown",
    host: { os: sw.stdout.trim().split("\n").map((l) => l.replace(/\s+/g, " ")).join("; "), arch: deps.arch, hostname: deps.hostname },
  };
  if (o.work) fs.mkdirSync(o.work, { recursive: true });
  base.work = fs.realpathSync(fs.mkdtempSync(path.join(o.work || os.tmpdir(), "citrate-macos-acc-")));
  base.logDirAbs = o.logDir ? path.resolve(o.logDir) : path.join(base.work, "logs");
  fs.mkdirSync(base.logDirAbs, { recursive: true });

  if (o.selfTest) return selfTest(base, o);

  const v = await oneRun(base, o, "acceptance", loadReleaseSpec());
  const out = o.out ? path.resolve(o.out) : path.join(base.logDirAbs, "verdict.json");
  fs.writeFileSync(out, JSON.stringify(v, null, 2) + "\n");
  deps.out(`verdict: ${v.overall} mode=${v.mode} g1_evidence=${v.g1_evidence} (${out})`);
  return exitCodeFor(v.overall);
}

async function selfTest(base, o) {
  const { deps, rn } = base;
  const port = 18545;
  const sc = buildStandInSidecars(base);
  const spec = { release: "0.0.0-selftest", bins: ["citrate", "ipfs"], errors: [],
    sidecars: [{ id: "stand-in-node", bin: "citrate", sha256: sc.sha }, { id: "stand-in-ipfs", bin: "ipfs", sha256: sc.sha }] };
  const smoke = { ...o, pinnedDir: sc.pinnedDir, unsignedSmoke: true, quitMethod: "term", rpc: false, required: ["citrate", "ipfs"], restartTarget: "citrate" };
  let allOk = true;
  const report = (ok, what) => { deps.out(`${ok ? "OK  " : "BAD "} self-test ${what}`); allOk = allOk && ok; };
  for (const m of SELF_TEST_MODES) {
    const { dmg, sha256 } = buildStandInDmg(base, m.mode, port, sc);
    const v = await oneRun(base, { ...smoke, dmg, sha256, mode: m.run }, `self-test-${m.mode}`, spec);
    const j = judgeSelfTest(m.expectFail, v);
    report(j.ok, `${m.mode} (${m.run}): overall=${v.overall} failed=[${j.failed.join(", ")}] want=[${m.expectFail.join(", ")}]`);
    if (m.mode === "stray-file") rn.mut("rm", ["-f", "--", `/private/tmp/citrate-selftest-stray-${port}`]);
    if (m.mode === "keychain") {
      // the stand-in's item may have landed in the user's own keychain: remove exactly that item
      while (rn.mut("security", ["delete-generic-password", "-s", "ai.citrate.selftest-undocumented", "-a", "selftest"]).code === 0) { /* again */ }
    }
    if (m.mode === "good") {
      // negative controls: release-mode verify of the ad hoc stand-in fails the signing checks; a
      // wrong sha256 fails inspect; inspect of the right one passes (and is never G1 evidence).
      const neg = await oneRun(base, { ...o, dmg, sha256, mode: "verify", pinnedDir: sc.pinnedDir, expectVersion: "0.0.0-selftest" }, "self-test-signing-negative", spec);
      const failed = neg.checks.filter((c) => c.status === "fail").map((c) => c.id);
      report(neg.overall === "fail" && ["sign.developer_id", "sign.sidecars", "gatekeeper.spctl", "notarization.staple"].every((x) => failed.includes(x)) && !neg.g1_evidence,
        `signing-negative: overall=${neg.overall} failed=[${failed.join(", ")}]`);
      const ins = await oneRun(base, { ...o, dmg, sha256, mode: "inspect" }, "self-test-inspect", spec);
      report(ins.overall === "pass" && !ins.g1_evidence && !ins.checks.some((c) => c.id === "dmg.mount"), `inspect: overall=${ins.overall} g1_evidence=${ins.g1_evidence} (no mount)`);
      const badSha = await oneRun(base, { ...o, dmg, sha256: "0".repeat(64), mode: "inspect" }, "self-test-inspect-wrong-sha", spec);
      report(badSha.overall === "fail" && badSha.checks.some((c) => c.id === "dmg.sha256" && c.status === "fail"), `inspect-wrong-sha: overall=${badSha.overall}`);
    }
    rn.mut("rm", ["-f", "--", dmg]);
  }
  rn.mut("rm", ["-rf", "--", sc.dir]);
  deps.out(allOk ? "SELF-TEST PASS" : "SELF-TEST FAIL");
  return allOk ? 0 : 1;
}

// ---------------------------------------------------------------- real dependencies
export function realDeps() {
  return {
    platform: process.platform, arch: process.arch, uid: process.getuid ? process.getuid() : -1,
    env: process.env, pwHome: os.userInfo().homedir, hostname: os.hostname(),
    exists: (p) => fs.existsSync(p),
    readHostsFile: (p) => {
      try {
        const st = fs.lstatSync(p);
        if (!st.isFile()) return null;
        return { uid: st.uid, mode: st.mode, text: fs.readFileSync(p, "utf8") };
      } catch { return null; }
    },
    exec: (cmd, args, o = {}) => {
      const r = spawnSync(cmd, args, { encoding: "utf8", cwd: o.cwd, timeout: o.timeout ?? 300000, maxBuffer: 256 * 1024 * 1024 });
      return { code: r.status ?? -1, stdout: r.stdout || "", stderr: r.stderr || (r.error ? String(r.error) : "") };
    },
    spawnApp: (rn, exe, env, logFile) => {
      if (!rn.guarded) throw new Error("refusing to launch before the machine-safety guards passed");
      const fd = fs.openSync(logFile, "a");
      const child = spawn(exe, [], { env, detached: true, stdio: ["ignore", fd, fd] });
      child.unref();
      fs.closeSync(fd);
      return child.pid;
    },
    kill: (pid, sig) => process.kill(pid, sig),
    out: (s) => process.stdout.write(s + "\n"),
    err: (s) => process.stderr.write(s + "\n"),
  };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2), realDeps()).then((c) => process.exit(c), (e) => { process.stderr.write(`${e.stack || e}\n`); process.exit(2); });
}
