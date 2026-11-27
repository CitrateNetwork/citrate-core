#!/usr/bin/env node
// created: 2026-11-27 | branch: feat/v60-lq-wp03-tripwire-inventory
// author: Claude Opus 5.5 (LQ-WP03 worker) for Larry Klosowski
// status: active
//
// LQ-WP03: the honesty and tripwire registry (.agentile/coverage/tripwires.json) and its check.
// A registered tripwire stays in the runners' own test lists and runs (a Rust test marked
// #[ignore] is listed but not run), or is replaced only by a stricter test: the old entry names
// `replaced_by`, the chain of replacements ends at a registered test that is listed and runs, each
// new edge links `replacement_evidence` (the run or commit where the replacement failed against a
// deliberately weakened copy of the guarded code), and the PR carries the `tripwire-replace` label.
// Entries are never removed and an earlier edge is never re-pointed: a replacement is itself
// retired by setting `replaced_by` on its own entry (A -> B, then B -> C).
// Rules and procedure: .agentile/coverage/TRIPWIRES.md.
//
// Data source (Rule 7): `cargo test --workspace --locked --offline -- --list` (stdout and stderr
// together, so cargo's "Running" lines name each test binary), the same with `--ignored` (only the
// #[ignore] tests), and `vitest list --json=<file>` (vitest leaves skipped, todo and only-filtered
// tests out of that listing itself).
//
// usage: tripwire-inventory.mjs <list|generate|check|count> [options]
//   --cargo-list <file>       cargo test -- --list log
//   --cargo-ignored <file>    cargo test -- --list --ignored log
//   --vitest-list <file>      vitest list --json output
//   --root <dir>              repo root vitest paths are made relative to (default: cwd)
//   --registry <file>         the registry (check, count; generate keeps its entries)
//   --base-registry <file>    the registry at the merge base (check)
//   --labels-json <json>      the PR's labels as a JSON array of names, matched exactly (check)
//   --no-labels               labels unavailable (merge_group, push); the PR run enforced them
//   --strict-unregistered     an unregistered tripwire-named test fails instead of warning
//   --since <rev>             `since` for entries generate adds
// exit: 0 pass, 1 refused, 2 usage or unusable input.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const CLASSES = [
  "custody", "ceremony", "money", "honesty", "sidecar", "updater", "network", "ipc", "privacy", "ui-truth",
];

// Name patterns from the LQ-WP03 spec. Rust names are snake_case; vitest titles are prose, so the
// vitest pattern reads the title words and the file name (a `*.tripwire.test.*` or `*Honesty*`
// file is a tripwire file as a whole).
const RUST_NAME = /tripwire|honest|never_|adv[0-9]|dark_refuses|payouts_dark/;
// Modules that are tripwires as a whole (IPC surface: removed commands, invoke secret scan).
const RUST_MODULE = /(^|::)(removed_commands_tests|invoke_secret_scan_tests)(::|$)/;
const VITEST_TITLE = /tripwire|honest|fabricat/i;
const VITEST_FILE = /tripwire|honesty/i;

// A link to the run or commit where the replacement failed against a weakened copy.
const EVIDENCE = /^https:\/\/github\.com\/CitrateNetwork\/[A-Za-z0-9._-]+\/(actions\/runs\/\d+(\/job\/\d+)?|commit\/[0-9a-f]{7,40}|pull\/\d+\/commits\/[0-9a-f]{7,40})$/;

export class UsageError extends Error {}

// ---- listings ----------------------------------------------------------------------------------

// `cargo test -- --list`: "     Running unittests src/lib.rs (target/debug/deps/citrate_core_kit-0a1b...)"
// then "<path>: test" lines. Unit = binary stem without the hash; a unittests binary built from
// anything but src/lib.rs gets "@bin", so a crate's lib and bin of the same name stay apart.
// Doc-tests are skipped (not tripwires, and their names carry line numbers).
export function parseCargoList(text) {
  return parseCargoListing(text).ids;
}

// The ids plus the number of test binaries the log ran ("Running" lines), so an ignored listing
// with no ignored tests can be told from a list step that never ran.
export function parseCargoListing(text) {
  const ids = new Set();
  let binaries = 0;
  let unit = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.replace(/\x1b\[[0-9;]*m/g, "");
    const running = line.match(/^\s*Running (unittests )?(\S+) \(([^)]+)\)\s*$/);
    if (running) {
      const stem = path.basename(running[3]).replace(/\.exe$/, "").replace(/-[0-9a-f]{16}$/, "");
      const bin = running[1] && running[2] !== "src/lib.rs";
      unit = bin ? `${stem}@bin` : stem;
      binaries += 1;
      continue;
    }
    if (/^\s*Doc-tests /.test(line)) {
      unit = null;
      continue;
    }
    const t = line.match(/^(\S+): test$/);
    if (t && unit) ids.add(`${unit}::${t[1]}`);
  }
  return { ids, binaries };
}

// `vitest list --json`: [{ name: "suite > test", file: "/abs/path" }, ...]. Two tests with the same
// title in one file are told apart by occurrence (" #2", " #3"), so deleting one of them shows.
export function parseVitestList(text, root) {
  let doc;
  try {
    doc = JSON.parse(text);
  } catch (e) {
    throw new UsageError(`vitest listing is not JSON: ${e.message}`);
  }
  const rows = Array.isArray(doc) ? doc : Array.isArray(doc?.tests) ? doc.tests : null;
  if (!rows) throw new UsageError("vitest listing is not an array of {name, file}");
  const ids = new Set();
  const seen = new Map();
  for (const r of rows) {
    if (!r || typeof r.name !== "string" || typeof r.file !== "string") {
      throw new UsageError("vitest listing row without name and file");
    }
    const rel = path.isAbsolute(r.file) ? path.relative(root, r.file) : r.file;
    const id = `vitest:${rel.split(path.sep).join("/")}::${r.name}`;
    const n = (seen.get(id) ?? 0) + 1;
    seen.set(id, n);
    ids.add(n === 1 ? id : `${id} #${n}`);
  }
  return ids;
}

function splitVitest(id) {
  const m = id.match(/^vitest:(.*?)::(.*)$/s);
  return m ? { file: m[1], name: m[2] } : null;
}

export function isTripwireName(id) {
  const v = splitVitest(id);
  if (v) return VITEST_FILE.test(path.posix.basename(v.file)) || VITEST_TITLE.test(v.name);
  const fn = id.split("::").pop();
  return RUST_NAME.test(fn) || RUST_MODULE.test(id);
}

// Advisory class tag (the review owner can retag; the check does not enforce classes).
export function classify(id) {
  const s = id.toLowerCase();
  const v = splitVitest(id);
  if (v) return /privacy|telemetry/.test(s) ? "privacy" : "ui-truth";
  if (/ceremony/.test(s)) return "ceremony";
  if (/custody|vault/.test(s)) return "custody";
  if (/removed_commands|invoke_secret|ipc/.test(s)) return "ipc";
  if (/updater|update_policy|release/.test(s)) return "updater";
  if (/telemetry|privacy|redact|scrub|pii/.test(s)) return "privacy";
  if (/budget|payout|dark_refuses|gas_limit|claim|nonce|signer|sign_|send|stake|bond|grant|proposer|money|wallet/.test(s)) return "money";
  if (/sidecar|vendor_key|key_tripwire|chat_route|hermes|agent|mcp|store_session|daemon|provider/.test(s)) return "sidecar";
  if (/network|fleet|mesh|relay|tailnet|peer|rpc|sync|invite/.test(s)) return "network";
  return "honesty";
}

// ---- registry ----------------------------------------------------------------------------------

function readRegistry(file, label) {
  let text;
  try {
    text = fs.readFileSync(file, "utf8");
  } catch {
    throw new UsageError(`${label} not readable: ${file}`);
  }
  try {
    return JSON.parse(text);
  } catch (e) {
    return { __parseError: e.message };
  }
}

// Returns a list of problems; empty when the registry is well formed.
export function validateRegistry(reg) {
  const errs = [];
  if (reg?.__parseError) return [`registry is not JSON: ${reg.__parseError}`];
  if (!reg || reg.version !== 1) errs.push("registry version must be 1");
  if (!Array.isArray(reg?.tripwires)) return [...errs, "registry.tripwires must be an array"];
  const seen = new Set();
  for (const [i, t] of reg.tripwires.entries()) {
    const at = `tripwires[${i}]`;
    if (!t || typeof t.id !== "string" || !t.id) {
      errs.push(`${at}: id must be a non-empty string`);
      continue;
    }
    if (seen.has(t.id)) errs.push(`duplicate registry id: ${t.id}`);
    seen.add(t.id);
    if (!CLASSES.includes(t.class)) errs.push(`${t.id}: class "${t.class}" is not one of ${CLASSES.join("|")}`);
    if (typeof t.since !== "string" || !t.since) errs.push(`${t.id}: since must be a revision string`);
    if (t.replaced_by !== null && (typeof t.replaced_by !== "string" || !t.replaced_by)) {
      errs.push(`${t.id}: replaced_by must be null or a test id`);
    }
  }
  return errs;
}

// `present` is the set of tests that run; `ignored` the listed tests that do not (#[ignore]).
export function check({ reg, base, present, ignored = new Set(), labels, strictUnregistered }) {
  const errors = [];
  const warnings = [];
  const notes = [];
  const bad = validateRegistry(reg);
  if (bad.length) return { errors: bad, warnings, notes };
  const head = new Map(reg.tripwires.map((t) => [t.id, t]));
  const baseMap = base ? new Map(base.tripwires.map((t) => [t.id, t])) : null;

  if (baseMap) {
    for (const [id, b] of baseMap) {
      const h = head.get(id);
      if (!h) {
        errors.push(`registry entry removed: ${id} (entries are never removed; set replaced_by to a stricter registered test)`);
        continue;
      }
      if (b.replaced_by && h.replaced_by !== b.replaced_by) {
        errors.push(`replaced_by changed for ${id}: ${b.replaced_by} -> ${h.replaced_by} (replace the replacement's own entry instead)`);
      }
    }
  }

  let active = 0;
  for (const [id, t] of head) {
    if (t.replaced_by === null) {
      active += 1;
      if (ignored.has(id)) {
        errors.push(`registered tripwire is ignored: ${id} (#[ignore] keeps it listed but ci.yml does not run it; remove the ignore or replace it)`);
      } else if (!present.has(id)) {
        errors.push(`missing registered tripwire: ${id} (deleted or renamed; a replacement needs replaced_by, label tripwire-replace and evidence)`);
      }
      continue;
    }
    const to = t.replaced_by;
    if (to === id) errors.push(`${id} is replaced by itself`);
    if (!head.has(to)) errors.push(`replacement is not registered: ${to} (add its own entry)`);
    if (typeof t.replacement_evidence !== "string" || !EVIDENCE.test(t.replacement_evidence)) {
      errors.push(`replacement_evidence must link the run or commit where ${to} failed against a weakened copy of the guarded code (${id})`);
    }
    const isNew = !baseMap || !baseMap.get(id)?.replaced_by;
    if (isNew) {
      if (labels === null) notes.push(`labels not available on this event; the PR check enforced tripwire-replace for ${id}`);
      else if (!labels.has("tripwire-replace")) errors.push(`needs the tripwire-replace label: ${id} -> ${to}`);
    }
    notes.push(`replaced: ${id} -> ${to}`);
  }

  // Every replacement chain ends at a registered, active entry whose test is listed and runs.
  // Only the end has to run: a middle link (B in A -> B -> C) is retired by its own edge.
  for (const [id, t] of head) {
    if (t.replaced_by === null || t.replaced_by === id) continue;
    const chain = [id];
    let cur = t;
    let broken = false;
    while (cur.replaced_by !== null) {
      if (chain.includes(cur.replaced_by)) {
        errors.push(`replacement cycle through ${id}`);
        broken = true;
        break;
      }
      chain.push(cur.replaced_by);
      cur = head.get(cur.replaced_by);
      if (!cur) {
        broken = true; // "replacement is not registered" is reported for the edge itself
        break;
      }
    }
    if (broken) continue;
    const end = chain[chain.length - 1];
    if (ignored.has(end)) errors.push(`replaced_by points to a test that is ignored: ${chain.join(" -> ")}`);
    else if (!present.has(end)) errors.push(`replaced_by points to a test that does not exist: ${chain.join(" -> ")}`);
  }

  for (const id of [...present].sort()) {
    if (head.has(id) || !isTripwireName(id)) continue;
    const msg = `unregistered tripwire: ${id} (add it to .agentile/coverage/tripwires.json)`;
    if (strictUnregistered) errors.push(msg);
    else warnings.push(msg);
  }

  if (!errors.length) notes.push(`ok: ${active} registered tripwires present`);
  return { errors, warnings, notes };
}

export function generate({ reg, present, since }) {
  const out = new Map();
  for (const t of reg?.tripwires ?? []) out.set(t.id, t);
  for (const id of present) {
    if (!out.has(id) && isTripwireName(id)) out.set(id, { id, class: classify(id), since, replaced_by: null });
  }
  return { version: 1, tripwires: [...out.values()].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)) };
}

// ---- CLI ---------------------------------------------------------------------------------------

const USAGE = "usage: tripwire-inventory.mjs <list|generate|check|count> [--cargo-list f --cargo-ignored f --vitest-list f] [--root d] [--registry f] [--base-registry f] [--labels-json '[\"a\"]' | --no-labels] [--strict-unregistered] [--since rev]";

function parseArgs(argv) {
  const [cmd, ...rest] = argv;
  if (!["list", "generate", "check", "count"].includes(cmd)) throw new UsageError(USAGE);
  const o = { cmd, root: process.cwd(), labels: undefined, strict: false, noLabels: false };
  const takes = { "--cargo-list": "cargo", "--cargo-ignored": "ignored", "--vitest-list": "vitest", "--root": "root", "--registry": "registry", "--base-registry": "base", "--labels-json": "labels", "--since": "since" };
  for (let i = 0; i < rest.length; i += 1) {
    const a = rest[i];
    if (a === "--strict-unregistered") o.strict = true;
    else if (a === "--no-labels") o.noLabels = true;
    else if (takes[a]) {
      if (i + 1 >= rest.length) throw new UsageError(`${a} needs a value\n${USAGE}`);
      o[takes[a]] = rest[(i += 1)];
    } else throw new UsageError(`unknown option ${a}\n${USAGE}`);
  }
  return o;
}

// Exact label names from the JSON array GitHub gives (toJSON(...labels.*.name)). Never split a
// string: a label name may itself contain a comma.
function parseLabels(text) {
  let v;
  try {
    v = JSON.parse(text);
  } catch {
    v = undefined;
  }
  if (!Array.isArray(v) || v.some((x) => typeof x !== "string")) {
    throw new UsageError(`--labels-json must be a JSON array of label names\n${USAGE}`);
  }
  return new Set(v);
}

function readLists(o) {
  if (!o.cargo || !o.vitest) throw new UsageError(`--cargo-list and --vitest-list are required\n${USAGE}`);
  if (!o.ignored) throw new UsageError(`--cargo-ignored is required (cargo test -- --list --ignored; an #[ignore]d test is listed but does not run)\n${USAGE}`);
  let cargoText, ignoredText, vitestText;
  try {
    cargoText = fs.readFileSync(o.cargo, "utf8");
    ignoredText = fs.readFileSync(o.ignored, "utf8");
    vitestText = fs.readFileSync(o.vitest, "utf8");
  } catch (e) {
    throw new UsageError(`listing not readable: ${e.message}`);
  }
  const listed = parseCargoList(cargoText);
  const ign = parseCargoListing(ignoredText);
  const vitest = parseVitestList(vitestText, o.root);
  if (!listed.size) throw new UsageError("no tests in the cargo listing (did the list step fail?)");
  if (!ign.binaries) throw new UsageError("no test binaries in the cargo ignored listing (did the --list --ignored step fail?)");
  for (const id of ign.ids) {
    if (!listed.has(id)) throw new UsageError(`the ignored listing names a test the full listing does not have: ${id} (both listings must come from one build)`);
  }
  if (!vitest.size) throw new UsageError("no tests in the vitest listing (did the list step fail?)");
  const rust = new Set([...listed].filter((id) => !ign.ids.has(id)));
  return { rust, ignored: ign.ids, vitest, present: new Set([...rust, ...vitest]) };
}

export function main(argv, io = { out: (s) => process.stdout.write(s), err: (s) => process.stderr.write(s) }) {
  try {
    const o = parseArgs(argv);
    if (o.cmd === "list") {
      const { rust, ignored, vitest } = readLists(o);
      io.out(`${JSON.stringify({ rust: [...rust].sort(), rust_ignored: [...ignored].sort(), vitest: [...vitest].sort() }, null, 2)}\n`);
      return 0;
    }
    if (o.cmd === "generate") {
      const { present } = readLists(o);
      const reg = o.registry ? readRegistry(o.registry, "registry") : null;
      if (reg) {
        const bad = validateRegistry(reg);
        if (bad.length) {
          for (const b of bad) io.err(`error: ${b}\n`);
          return 1;
        }
      }
      io.out(`${JSON.stringify(generate({ reg, present, since: o.since ?? "unknown" }), null, 2)}\n`);
      return 0;
    }
    if (!o.registry) throw new UsageError(`--registry is required\n${USAGE}`);
    if (o.cmd === "count") {
      const reg = readRegistry(o.registry, "registry");
      const bad = validateRegistry(reg);
      if (bad.length) {
        for (const b of bad) io.err(`error: ${b}\n`);
        return 1;
      }
      const lists = o.cargo && o.vitest ? readLists(o) : null;
      const by_class = {};
      for (const t of reg.tripwires) by_class[t.class] = (by_class[t.class] ?? 0) + 1;
      const active = reg.tripwires.filter((t) => t.replaced_by === null);
      const c = { registered: reg.tripwires.length, active: active.length, replaced: reg.tripwires.length - active.length, by_class };
      if (lists) c.present = active.filter((t) => lists.present.has(t.id)).length;
      io.out(`${JSON.stringify(c)}\n`);
      return 0;
    }
    // check
    if (o.labels === undefined && !o.noLabels) throw new UsageError(`check needs --labels-json (may be []) or --no-labels\n${USAGE}`);
    const labels = o.noLabels ? null : parseLabels(o.labels);
    const { present, ignored } = readLists(o);
    const reg = readRegistry(o.registry, "registry");
    let base = null;
    if (o.base) {
      base = readRegistry(o.base, "base registry");
      const bad = validateRegistry(base);
      if (bad.length) throw new UsageError(`base registry is malformed: ${bad[0]}`);
    }
    const r = check({ reg, base, present, ignored, labels, strictUnregistered: o.strict });
    for (const n of r.notes) io.out(`${n}\n`);
    for (const w of r.warnings) io.out(`warning: ${w}\n`);
    for (const e of r.errors) io.err(`error: ${e}\n`);
    if (r.errors.length) {
      io.err(`lq / tripwires: refused (${r.errors.length}). See .agentile/coverage/TRIPWIRES.md.\n`);
      return 1;
    }
    return 0;
  } catch (e) {
    if (e instanceof UsageError) {
      io.err(`${e.message}\n`);
      return 2;
    }
    throw e;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
