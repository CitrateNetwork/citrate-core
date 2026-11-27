#!/usr/bin/env node
// lq / chain-touch (LQ-WP04, planset 2026-11-07-v060 lanes/LQ_NON_REGRESSION_GUARD.md): the CI guard
// that keeps 0.6.0 product lanes off chain-adjacent code. It reads the PR's diff against its base,
// classifies each changed path with release/chain-touch.json, and enforces:
//
//   1. A product-lane branch (lane_branch_pattern, feat/v60-l1..l9-*) that changes a class C path
//      fails: the lane files a request to L0 or the chain lane instead.
//   2. Any other class C change must be a chain-touch PR: from a chain_touch_branches prefix
//      (feat/v60-chain-*, feat/v60-l0-*), labelled chain-touch, with
//      release/evidence/chain/<rev12>/{source-fingerprint.json, agreement-x86_64.json,
//      agreement-aarch64.json, bench.md} for every chain rev it pins, a green `lq / chain-agreement`
//      run on the head (from the github-actions app, and from the workflow file agreement_workflow
//      as the base branch carries it: a same-named job in any other workflow does not count), and
//      for a rules delta (rules fingerprint changed from the base's record, no
//      base record, or a classes.C.rules_paths file changed) the consensus-review label plus an
//      APPROVE on the head commit from release/consensus-reviewers.json.
//   3. A Cargo.lock change that moves any package in the closure of the chain crates is class C.
//   4. A new or moved citrate-node source.target_revs entry in release/components.json is refused
//      for 0.6.0 (one node rev on every OS). Removing one is allowed.
//
// The map and the reviewer set are read from the base branch TIP (--base), so a PR cannot change its
// own guard and a long-lived branch is judged by the current map (a PR that adds the map, when the
// base has none, is checked with its own copy; the reviewer set never falls back to the head, so a
// rules delta fails closed until the base carries it). The diff, and the base node rev a rules delta is measured
// from, use the merge-base of the tip and the head. The check never
// writes: it reads git, the event payload and the read-only GitHub API. Anything it cannot read
// (map, lockfile, components.json, API) fails closed.
//
// Data sources (Rule 7): `git diff --name-only <merge-base>..HEAD`; `git show <ref>:<path>`;
// release/chain-touch.json; release/consensus-reviewers.json; the PR's labels, reviews, the head's
// check runs and the workflow run behind each one's check suite (GitHub REST, read-only token). Cargo.lock closures come from the lockfile's own
// resolved graph (every [[package]] and its dependency list), which needs no registry or git fetch.
//
// Usage:
//   node scripts/ci/lq/chain-touch.mjs check --base <ref> [--labels a,b] [--branch <name>]
//        [--head <ref>] [--head-sha <sha>] [--reviews <file.json>] [--checks <file.json>]
//        [--workflow-runs <file.json: { "<check suite id>": [workflow runs] }>] [--root <dir>]
//   node scripts/ci/lq/chain-touch.mjs check --from-event [--base <ref>]   (GitHub Actions)
//   node scripts/ci/lq/chain-touch.mjs --self-test
// Exit: 0 pass, 1 refused (or unreadable input: fail closed), 2 usage (bad flag or unknown ref).
//
// Zero dependencies: node built-ins and git.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const MAP_PATH = "release/chain-touch.json";
export const REVIEWERS_PATH = "release/consensus-reviewers.json";
const HEX40 = /^[0-9a-f]{40}$/;

class UsageError extends Error {}

// Text that came from the PR (paths, branch names, labels) is printed with control characters
// replaced, so it can never start a line and so never forms a workflow command.
export function clean(s) {
  return String(s).replace(/[\u0000-\u001f\u007f]/g, "?");
}

export function globToRegExp(glob) {
  let re = "";
  for (let i = 0; i < glob.length; ) {
    if (glob.startsWith("**/", i)) { re += "(?:.*/)?"; i += 3; continue; }
    if (glob.startsWith("**", i)) { re += ".*"; i += 2; continue; }
    const ch = glob[i++];
    if (ch === "*") re += "[^/]*";
    else if (ch === "?") re += "[^/]";
    else re += ch.replace(/[.+^${}()|[\]\\]/g, "\\$&");
  }
  if (glob.endsWith("/")) re += ".*";
  return new RegExp(`^${re}$`);
}

export function matchPath(pattern, file) {
  if (!/[*?]/.test(pattern)) return pattern.endsWith("/") ? file.startsWith(pattern) : file === pattern;
  return globToRegExp(pattern).test(file);
}

// The part of one TOML line before its comment. A '#' inside a basic ("...") or literal ('...')
// string is not a comment. Unbalanced quotes keep the whole line (the strict side).
export function stripTomlComment(line) {
  let q = null;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (q === '"') { if (ch === "\\") i++; else if (ch === '"') q = null; continue; }
    if (q === "'") { if (ch === "'") q = null; continue; }
    if (ch === '"' || ch === "'") { q = ch; continue; }
    if (ch === "#") return line.slice(0, i).trimEnd();
  }
  return line;
}

const scopedLines = (s, text) => (text ?? "").split(/\r?\n/).map((l) => (s.comments === "toml" ? stripTomlComment(l) : l));

// ---- git -------------------------------------------------------------------------------------

function git(root, args) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 512 * 1024 * 1024 });
}

function resolveRef(root, ref) {
  try {
    return git(root, ["rev-parse", "--verify", "--quiet", `${ref}^{commit}`]).trim();
  } catch {
    throw new UsageError(`unknown ref '${clean(ref)}'`);
  }
}

export function readAt(root, sha, file) {
  try {
    return git(root, ["show", `${sha}:${file}`]);
  } catch {
    return null;
  }
}

function changedFiles(root, base, head) {
  return git(root, ["diff", "--name-only", "--no-renames", "-z", base, head]).split("\0").filter(Boolean);
}

// ---- Cargo.lock ------------------------------------------------------------------------------

export function parseLock(text) {
  const pk = [];
  let cur = null;
  let inDeps = false;
  const quoted = (s) => [...s.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (inDeps) {
      if (line === "]") { inDeps = false; continue; }
      const m = line.match(/^"([^"]+)",?$/);
      if (!m) throw new Error(`Cargo.lock: bad dependency line '${clean(line).slice(0, 80)}'`);
      cur.dependencies.push(m[1]);
      continue;
    }
    if (line === "[[package]]") {
      cur = { name: null, version: null, source: null, checksum: null, dependencies: [] };
      pk.push(cur);
      continue;
    }
    if (line.startsWith("[")) { cur = null; continue; }
    if (!cur || line === "" || line.startsWith("#")) continue;
    const kv = line.match(/^(name|version|source|checksum)\s*=\s*"([^"]*)"$/);
    if (kv) { cur[kv[1]] = kv[2]; continue; }
    if (/^dependencies\s*=\s*\[\s*$/.test(line)) { inDeps = true; continue; }
    const inline = line.match(/^dependencies\s*=\s*\[(.*)\]$/);
    if (inline) { cur.dependencies.push(...quoted(inline[1])); continue; }
  }
  if (inDeps) throw new Error("Cargo.lock: unterminated dependencies list");
  for (const p of pk) if (!p.name || !p.version) throw new Error("Cargo.lock: a [[package]] without name or version");
  return pk;
}

function resolveDep(byName, dep) {
  const m = dep.match(/^(\S+)(?: (\S+))?(?: \((.+)\))?$/);
  if (!m) throw new Error(`Cargo.lock: bad dependency '${clean(dep)}'`);
  const [, name, version, source] = m;
  const hits = (byName.get(name) || []).filter((p) => (!version || p.version === version) && (!source || p.source === source));
  if (hits.length !== 1) throw new Error(`Cargo.lock: dependency '${clean(dep)}' resolves to ${hits.length} packages`);
  return hits[0];
}

// The packages reachable from the roots (by name, or any package whose source matches
// sourceMatch), each as one line "<name> <version> <source> <checksum> [deps]", sorted.
export function lockClosure(text, roots, sourceMatch) {
  const pk = parseLock(text);
  const byName = new Map();
  for (const p of pk) byName.set(p.name, [...(byName.get(p.name) || []), p]);
  const src = sourceMatch ? new RegExp(sourceMatch, "i") : null;
  const queue = pk.filter((p) => roots.includes(p.name) || (src && p.source && src.test(p.source)));
  const seen = new Set();
  while (queue.length) {
    const p = queue.pop();
    if (seen.has(p)) continue;
    seen.add(p);
    for (const d of p.dependencies) queue.push(resolveDep(byName, d));
  }
  return [...seen].map((p) => `${p.name} ${p.version} ${p.source || "-"} ${p.checksum || "-"} [${p.dependencies.join(",")}]`).sort();
}

// ---- classification --------------------------------------------------------------------------

function canonical(v) {
  if (Array.isArray(v)) return `[${v.map(canonical).join(",")}]`;
  if (v && typeof v === "object") return `{${Object.keys(v).sort().map((k) => `${JSON.stringify(k)}:${canonical(v[k])}`).join(",")}}`;
  return JSON.stringify(v === undefined ? null : v);
}

function jsonEntry(text, s) {
  if (text === null) return null;
  const doc = JSON.parse(text);
  const arr = doc && Array.isArray(doc[s.array]) ? doc[s.array] : [];
  return arr.find((e) => e && e[s.key] === s.value) ?? null;
}

// Returns a short detail string when the scoped part changed, else null. Unreadable: changed.
function scopedChange(s, baseText, headText) {
  try {
    if (s.kind === "lines") {
      const re = new RegExp(s.match, "i");
      const pick = (t) => scopedLines(s, t).filter((l) => re.test(l)).sort();
      const a = pick(baseText), b = pick(headText);
      if (a.join("\n") === b.join("\n")) return null;
      return `${a.length} matching line(s) at base, ${b.length} at head, contents differ`;
    }
    if (s.kind === "json-entry") {
      const a = canonical(jsonEntry(baseText, s)), b = canonical(jsonEntry(headText, s));
      return a === b ? null : `${s.array}[${s.key}=${s.value}] changed`;
    }
    if (s.kind === "lock-closure") {
      const a = baseText === null ? [] : lockClosure(baseText, s.roots || [], s.source_match);
      const b = headText === null ? [] : lockClosure(headText, s.roots || [], s.source_match);
      const A = new Set(a), B = new Set(b);
      const moved = [...new Set([...a.filter((x) => !B.has(x)), ...b.filter((x) => !A.has(x))].map((x) => x.split(" ").slice(0, 2).join(" ")))];
      return moved.length ? `closure of ${(s.roots || []).join(", ")} moved: ${moved.slice(0, 6).join("; ")}${moved.length > 6 ? "; ..." : ""}` : null;
    }
    return `unknown scoped kind '${clean(s.kind)}' (fail closed)`;
  } catch (e) {
    return `unreadable at base or head (fail closed): ${clean(e.message).slice(0, 160)}`;
  }
}

export function classify(map, files, readBase, readHead) {
  const C = map.classes.C;
  const out = [];
  for (const f of files) {
    if (C.paths.some((p) => matchPath(p, f))) { out.push({ path: f, why: C.why }); continue; }
    for (const s of C.scoped || []) {
      if (!matchPath(s.path, f)) continue;
      const detail = scopedChange(s, readBase(f), readHead(f));
      if (detail) { out.push({ path: f, why: s.why || C.why, detail }); break; }
    }
  }
  return out;
}

export function validateMap(m) {
  const errs = [];
  const strs = (v) => Array.isArray(v) && v.every((x) => typeof x === "string" && x.length);
  if (!m || m.version !== 1) errs.push("version must be 1");
  const C = m?.classes?.C;
  if (!C || !strs(C.paths)) errs.push("classes.C.paths must be a list of paths");
  if (C && C.scoped !== undefined && !(Array.isArray(C.scoped) && C.scoped.every((s) => s && typeof s.path === "string" && typeof s.kind === "string"))) errs.push("classes.C.scoped must be a list of { path, kind }");
  if (C && !strs(C.rules_paths ?? [])) errs.push("classes.C.rules_paths must be a list of paths");
  if (!strs(m?.chain_touch_branches)) errs.push("chain_touch_branches must be a list of branch prefixes");
  try { new RegExp(m?.lane_branch_pattern); if (typeof m?.lane_branch_pattern !== "string") throw new Error(); } catch { errs.push("lane_branch_pattern must be a regular expression"); }
  if (typeof m?.evidence?.dir !== "string" || !strs(m?.evidence?.required)) errs.push("evidence.dir and evidence.required are required");
  if (typeof m?.agreement_check !== "string") errs.push("agreement_check is required");
  if (typeof m?.agreement_workflow !== "string" || !/^\.github\/workflows\/[\w.-]+\.ya?ml$/.test(m.agreement_workflow)) errs.push("agreement_workflow must be a .github/workflows/<file>.yml path");
  if (typeof m?.agreement_app?.slug !== "string" || !Number.isInteger(m?.agreement_app?.id)) errs.push("agreement_app must be { slug, id } (the GitHub App that runs the agreement workflow)");
  for (const s of C?.scoped || []) if (s.comments !== undefined && s.comments !== "toml") errs.push(`classes.C.scoped ${s.path}: comments must be "toml"`);
  if (typeof m?.labels?.chain_touch !== "string" || typeof m?.labels?.consensus_review !== "string") errs.push("labels.chain_touch and labels.consensus_review are required");
  if (typeof m?.node_component !== "string") errs.push("node_component is required");
  for (const s of C?.scoped || []) { try { if (s.match) new RegExp(s.match, "i"); if (s.source_match) new RegExp(s.source_match, "i"); } catch { errs.push(`classes.C.scoped ${s.path}: bad regular expression`); } }
  return errs;
}

// ---- policies --------------------------------------------------------------------------------

function nodeSource(text, id) {
  if (text === null) return null;
  const c = (JSON.parse(text).components || []).find((x) => x && x.id === id);
  return c && c.source ? c.source : null;
}

export function targetRevErrors(baseText, headText, id) {
  let a, b;
  try { a = nodeSource(baseText, id)?.target_revs || {}; b = nodeSource(headText, id)?.target_revs || {}; } catch (e) {
    return [`release/components.json unreadable (fail closed): ${clean(e.message).slice(0, 160)}`];
  }
  const errs = [];
  for (const [t, v] of Object.entries(b)) {
    const rev = v && typeof v === "object" ? v.rev : v;
    const old = a[t] && typeof a[t] === "object" ? a[t].rev : a[t];
    if (rev !== old) errs.push(`per-target node rev ${clean(t)} ${clean(String(rev).slice(0, 12))}: refused for 0.6.0 (one node rev on every OS; remove target_revs, do not add or move one)`);
  }
  return errs;
}

function frontmatterOk(text) {
  const m = text.match(/^---\r?\n([\s\S]*?)\r?\n---/);
  return !!m && ["created", "branch", "author", "status"].every((k) => new RegExp(`^${k}:\\s*\\S`, "m").test(m[1]));
}

function latestApprovals(reviews, headSha) {
  const last = new Map();
  for (const r of [...reviews].sort((x, y) => String(x.submitted_at).localeCompare(String(y.submitted_at)))) {
    const login = r?.user?.login;
    if (!login || r.state === "COMMENTED" || r.state === "PENDING") continue;
    last.set(login.toLowerCase(), r);
  }
  return new Set([...last].filter(([, r]) => r.state === "APPROVED" && r.commit_id === headSha).map(([k]) => k));
}

// ---- the check -------------------------------------------------------------------------------

const asFn = (v) => (typeof v === "function" ? v : async () => v || []);

export async function runCheck(opts) {
  const root = opts.root || process.cwd();
  const head = resolveRef(root, opts.head || "HEAD");
  const baseTip = resolveRef(root, opts.base);
  const base = git(root, ["merge-base", baseTip, head]).trim();
  const headSha = opts.headSha || head;
  const readBase = (f) => readAt(root, base, f);
  const readTip = (f) => readAt(root, baseTip, f);
  const readHead = (f) => readAt(root, head, f);
  const errors = [];
  const notes = [];
  const res = (extra) => ({ ok: errors.length === 0, errors, notes, base, head, classC: [], rulesDelta: false, ...extra });

  // The map: the base branch tip's copy; the head's only when the base has none (the PR that
  // introduces it).
  let mapText = readTip(MAP_PATH);
  if (mapText === null) {
    mapText = readHead(MAP_PATH);
    if (mapText === null) { errors.push(`${MAP_PATH} missing at base and head (fail closed)`); return res(); }
    notes.push(`${MAP_PATH} is new in this change; checked with the head copy`);
  }
  let map;
  try { map = JSON.parse(mapText); } catch (e) { errors.push(`${MAP_PATH} unreadable (fail closed): ${clean(e.message)}`); return res(); }
  const mapErrs = validateMap(map);
  if (mapErrs.length) { for (const e of mapErrs) errors.push(`${MAP_PATH}: ${e} (fail closed)`); return res(); }

  const files = changedFiles(root, base, head);
  const classC = classify(map, files, readBase, readHead);
  const branchRaw = String(opts.branch || "");
  const branch = clean(branchRaw);
  const labels = new Set((opts.labels || []).map(String));
  const isChainBranch = map.chain_touch_branches.some((p) => branchRaw.startsWith(p));
  const isLane = !isChainBranch && new RegExp(map.lane_branch_pattern).test(branchRaw);
  const ctLabel = labels.has(map.labels.chain_touch);
  const prefixes = map.chain_touch_branches.map((p) => `${p}*`).join(" or ");
  const detail = (c) => (c.detail ? ` (${clean(c.detail)})` : "");

  // Policy 4: one node rev on every OS.
  if (files.includes("release/components.json")) errors.push(...targetRevErrors(readBase("release/components.json"), readHead("release/components.json"), map.node_component));

  // Policies 1 and 2 (routing).
  for (const c of classC) {
    if (isLane) errors.push(`class C path ${clean(c.path)}: file a request to L0/chain lane; product lanes never change chain-adjacent files${detail(c)}`);
    else if (!isChainBranch) errors.push(`class C path ${clean(c.path)}: only a chain-touch PR from ${prefixes} may change it (labelled ${map.labels.chain_touch}, with evidence)${detail(c)}`);
  }
  if (isChainBranch && classC.length && !ctLabel) errors.push(`class C change on ${branch}: label ${map.labels.chain_touch} and add the evidence (${classC.map((c) => clean(c.path)).join(", ")})`);

  let rulesDelta = false;
  if (ctLabel || (isChainBranch && classC.length)) {
    // Evidence for every chain rev this head pins: the node rev, and any chain git dep rev on a
    // changed chain dependency line.
    let baseNode = null, headNode = null;
    try { baseNode = nodeSource(readBase("release/components.json"), map.node_component); headNode = nodeSource(readHead("release/components.json"), map.node_component); } catch (e) {
      errors.push(`release/components.json unreadable (fail closed): ${clean(e.message).slice(0, 160)}`);
    }
    const revs = new Set();
    if (headNode && HEX40.test(headNode.rev || "")) revs.add(headNode.rev);
    else errors.push(`release/components.json: ${map.node_component} source.rev missing or not a 40-hex rev at head (fail closed)`);
    const depRe = new RegExp((map.classes.C.scoped || []).find((s) => s.path === "**/Cargo.toml")?.match || "citrate-chain", "i");
    for (const c of classC.filter((x) => matchPath("**/Cargo.toml", x.path))) {
      for (const line of scopedLines({ comments: "toml" }, readHead(c.path)).filter((l) => depRe.test(l))) {
        const m = line.match(/\brev\s*=\s*"([^"]*)"/);
        if (!m) continue;
        if (HEX40.test(m[1])) revs.add(m[1]);
        else errors.push(`${clean(c.path)}: chain git dependency rev '${clean(m[1])}' must be a full 40-hex rev`);
      }
    }
    const fpOf = (read, rev) => {
      const t = read(`${map.evidence.dir}${rev.slice(0, 12)}/source-fingerprint.json`);
      try { return t === null ? null : JSON.parse(t); } catch { return null; }
    };
    for (const rev of revs) {
      const dir = `${map.evidence.dir}${rev.slice(0, 12)}/`;
      for (const f of map.evidence.required) {
        const t = readHead(dir + f);
        if (t === null) { errors.push(`evidence missing: ${dir}${f}`); continue; }
        if (f.endsWith(".json")) {
          let j;
          try { j = JSON.parse(t); } catch { errors.push(`evidence unreadable: ${dir}${f}`); continue; }
          if (typeof j.rev !== "string" || !(j.rev === rev || (j.rev.length >= 12 && rev.startsWith(j.rev)))) errors.push(`evidence ${dir}${f}: rev ${clean(String(j.rev))} is not the pinned rev ${rev}`);
          if (f === "source-fingerprint.json" && (typeof j.rules_fp !== "string" || !j.rules_fp)) errors.push(`evidence ${dir}${f}: rules_fp missing`);
          const arch = f.match(/^agreement-(.+)\.json$/)?.[1];
          if (arch && j.arch !== undefined && j.arch !== arch) errors.push(`evidence ${dir}${f}: arch ${clean(String(j.arch))} is not ${arch}`);
        } else if (f.endsWith(".md") && !frontmatterOk(t)) {
          errors.push(`evidence ${dir}${f}: Rule-5 frontmatter (created, branch, author, status) missing`);
        }
      }
    }

    // Rules delta: compared with the BASE commit's fingerprint record for the base node rev, so a
    // PR cannot plant the record it is compared with.
    const reasons = [];
    const touchedRules = files.filter((f) => (map.classes.C.rules_paths || []).some((p) => matchPath(p, f)));
    if (touchedRules.length) reasons.push(`changes ${touchedRules.map(clean).join(", ")}`);
    const baseRev = baseNode && HEX40.test(baseNode.rev || "") ? baseNode.rev : null;
    for (const rev of revs) {
      if (rev === baseRev) continue;
      const before = baseRev ? fpOf(readBase, baseRev) : null;
      const after = fpOf(readHead, rev);
      if (!before) reasons.push(`no source fingerprint for the base rev ${baseRev ? baseRev.slice(0, 12) : "(unknown)"} at base (fail closed)`);
      else if (!after || after.rules_fp !== before.rules_fp) reasons.push(`rules fingerprint ${rev.slice(0, 12)} differs from base ${baseRev.slice(0, 12)}`);
    }
    rulesDelta = reasons.length > 0;
    if (rulesDelta) {
      notes.push(`rules delta: ${reasons.join("; ")}`);
      if (!labels.has(map.labels.consensus_review)) errors.push(`rules delta (${reasons.join("; ")}): label ${map.labels.consensus_review} is required`);
      // The base tip's set only, never the head's: a PR that adds the file would name its own
      // approver. Until L0 lands the file on the base, every rules delta fails closed.
      let reviewers = [];
      const rText = readTip(REVIEWERS_PATH);
      if (rText === null) errors.push(`${REVIEWERS_PATH} is not on the base branch (fail closed): a rules delta needs the consensus reviewer set the base already carries; a PR cannot supply its own`);
      else {
        try { reviewers = JSON.parse(rText).reviewers; if (!Array.isArray(reviewers)) throw new Error("reviewers must be a list"); } catch (e) {
          reviewers = [];
          errors.push(`${REVIEWERS_PATH} unreadable (fail closed): ${clean(e.message)}`);
        }
      }
      const set = new Set(reviewers.map((r) => String(r).toLowerCase()));
      const approved = [...latestApprovals(await asFn(opts.reviews)(), headSha)].filter((l) => set.has(l));
      if (!approved.length) errors.push(`rules delta: needs an APPROVE on head ${headSha.slice(0, 12)} from a consensus reviewer (${REVIEWERS_PATH} at base: ${reviewers.map(clean).join(", ") || "none"})`);
      else notes.push(`consensus approval: ${approved.join(", ")}`);
    }

    // A green agreement run on the head. Under pull_request a PR can add a workflow of its own with
    // a job of the same name, so a run counts only when it comes from the agreement app and its
    // check suite resolves to a run of agreement_workflow on this head, and only when that workflow
    // file is on the base branch and this PR leaves it as the base has it (the run executes the
    // head's copy).
    const wf = map.agreement_workflow;
    const wfTip = readTip(wf);
    if (wfTip === null) errors.push(`${wf} is not on the base branch: the agreement run must come from a workflow the base already carries (fail closed)`);
    else if (readHead(wf) !== wfTip) errors.push(`${wf} differs from the base branch: this PR's run of it would judge itself; land the workflow change in its own PR first`);
    const workflowRunsFor = typeof opts.workflowRuns === "function" ? opts.workflowRuns : async (id) => (opts.workflowRuns || {})[id] || [];
    const named = (await asFn(opts.checkRuns)(map.agreement_check)).filter((r) => r && r.name === map.agreement_check);
    const runs = [];
    let foreign = 0;
    for (const r of named) {
      const suite = r.check_suite?.id;
      const fromApp = r.app?.slug === map.agreement_app.slug && r.app?.id === map.agreement_app.id;
      const wrs = fromApp && Number.isInteger(suite) ? await workflowRunsFor(suite) : [];
      const fromWorkflow = Array.isArray(wrs) && wrs.length > 0 && wrs.every((w) => w && String(w.path ?? "").replace(/@.*$/, "") === wf && w.head_sha === headSha);
      if (fromWorkflow) runs.push(r);
      else foreign++;
    }
    if (foreign) notes.push(`ignored ${foreign} '${map.agreement_check}' check run(s) not from ${wf} (app ${map.agreement_app.slug}) on this head`);
    const done = runs.filter((r) => r.status === "completed" && r.conclusion !== "cancelled").sort((x, y) => String(x.completed_at).localeCompare(String(y.completed_at)));
    const lastRun = done[done.length - 1];
    if (runs.some((r) => r.status !== "completed")) errors.push(`${map.agreement_check} is still running on head ${headSha.slice(0, 12)}; re-run this check when it is green`);
    else if (!lastRun || lastRun.conclusion !== "success") errors.push(`needs a green ${map.agreement_check} run from ${wf} (app ${map.agreement_app.slug}) on head ${headSha.slice(0, 12)} (${lastRun ? `latest: ${clean(lastRun.conclusion)}` : "none found"})`);
  }

  return res({ classC, rulesDelta, files, branch, labels: [...labels].map(clean) });
}

// ---- GitHub (event mode) ---------------------------------------------------------------------

function api(env) {
  const token = env.GH_TOKEN || env.GITHUB_TOKEN;
  const baseUrl = (env.GITHUB_API_URL || "https://api.github.com").replace(/\/$/, "");
  const repo = env.GITHUB_REPOSITORY || "";
  if (!token) throw new Error("no GH_TOKEN/GITHUB_TOKEN for the read-only GitHub API (fail closed)");
  if (!/^[\w.-]+\/[\w.-]+$/.test(repo)) throw new Error("GITHUB_REPOSITORY is not owner/name");
  const get = async (url) => {
    const r = await fetch(url.startsWith("http") ? url : `${baseUrl}${url}`, { headers: { Authorization: `Bearer ${token}`, Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" } });
    if (!r.ok) throw new Error(`GitHub API ${r.status} on ${url.replace(baseUrl, "").split("?")[0]} (fail closed)`);
    return { body: await r.json(), next: (r.headers.get("link") || "").match(/<([^>]+)>;\s*rel="next"/)?.[1] };
  };
  const paged = async (url, pick = (b) => b) => {
    const out = [];
    for (let i = 0, u = url; u && i < 20; i++) { const { body, next } = await get(u); out.push(...pick(body)); u = next && next.startsWith(baseUrl) ? next : null; }
    return out;
  };
  return { repo, get: async (u) => (await get(u)).body, paged };
}

async function eventContext(env) {
  const name = env.GITHUB_EVENT_NAME;
  if (!env.GITHUB_EVENT_PATH) throw new UsageError("--from-event needs GITHUB_EVENT_PATH");
  const ev = JSON.parse(fs.readFileSync(env.GITHUB_EVENT_PATH, "utf8"));
  const gh = api(env);
  let pr, base;
  if (name === "pull_request" || name === "pull_request_review") {
    pr = ev.pull_request;
    base = pr?.base?.sha;
  } else if (name === "merge_group") {
    const m = /\/pr-(\d+)-[0-9a-f]+$/.exec(ev.merge_group?.head_ref || "");
    if (!m) throw new Error("merge_group head_ref names no PR (fail closed)");
    pr = await gh.get(`/repos/${gh.repo}/pulls/${m[1]}`);
    base = "HEAD^1"; // the group commit's parent: main plus the entries ahead of this one
  } else {
    throw new UsageError(`unsupported event '${clean(name)}'`);
  }
  if (!pr || !pr.head || !HEX40.test(pr.head.sha || "") || !Number.isInteger(pr.number)) throw new Error("event has no pull request (fail closed)");
  return {
    base,
    branch: pr.head.ref,
    labels: (pr.labels || []).map((l) => l.name),
    headSha: pr.head.sha,
    reviews: () => gh.paged(`/repos/${gh.repo}/pulls/${pr.number}/reviews?per_page=100`),
    checkRuns: (check) => gh.paged(`/repos/${gh.repo}/commits/${pr.head.sha}/check-runs?check_name=${encodeURIComponent(check)}&filter=all&per_page=100`, (b) => b.check_runs || []),
    workflowRuns: (suiteId) => gh.paged(`/repos/${gh.repo}/actions/runs?check_suite_id=${Number(suiteId)}&per_page=100`, (b) => b.workflow_runs || []),
  };
}

// ---- self-test -------------------------------------------------------------------------------

export function selfTest() {
  const failures = [];
  const lines = [];
  const cases = [
    ["src-tauri/src/node.rs", "src-tauri/src/node.rs", true],
    ["src-tauri/src/node.rs", "src-tauri/src/node_genesis.rs", false],
    ["core/consensus/", "core/consensus/src/lib.rs", true],
    ["core/consensus/", "core/consensus-ext/lib.rs", false],
    ["node/src/finality_*.rs", "node/src/finality_gadget.rs", true],
    ["node/src/finality_*.rs", "node/src/finality/x.rs", false],
    ["**/Cargo.toml", "Cargo.toml", true],
    ["**/Cargo.toml", "kit/Cargo.toml", true],
    ["**/Cargo.toml", "Cargo.toml.bak", false],
    ["**/.cargo/config.toml", "src-tauri/.cargo/config.toml", true],
    ["**/.cargo/config", "src-tauri/.cargo/config.toml", false],
  ];
  for (const [p, f, want] of cases) {
    const got = matchPath(p, f);
    (got === want ? lines : failures).push(`${p} ~ ${f} -> ${got} (want ${want})`);
  }
  for (const [line, want] of [['a = "x#y" # c', 'a = "x#y"'], ["# citrate-chain", ""]]) {
    const got = stripTomlComment(line);
    (got === want ? lines : failures).push(`toml comment ${line} -> ${got} (want ${want})`);
  }
  const lock = '[[package]]\nname = "citrate-wallet-core"\nversion = "0.4.0"\nsource = "git+https://github.com/CitrateNetwork/citrate-chain.git#x"\ndependencies = [\n "k256",\n]\n\n[[package]]\nname = "k256"\nversion = "0.13.4"\n\n[[package]]\nname = "serde"\nversion = "1.0.0"\n';
  const cl = lockClosure(lock, ["citrate-wallet-core"], "citrate-chain").map((l) => l.split(" ")[0]);
  (cl.join(",") === "citrate-wallet-core,k256" ? lines : failures).push(`lock closure -> ${cl.join(",")} (want citrate-wallet-core,k256)`);
  const mapFile = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "..", MAP_PATH);
  if (fs.existsSync(mapFile)) {
    let errs;
    try { errs = validateMap(JSON.parse(fs.readFileSync(mapFile, "utf8"))); } catch (e) { errs = [e.message]; }
    (errs.length ? failures : lines).push(`${MAP_PATH} ${errs.length ? `invalid: ${errs.join("; ")}` : "valid"}`);
  }
  return { failures, lines };
}

// ---- CLI -------------------------------------------------------------------------------------

function parseArgs(argv) {
  const o = { labels: [] };
  const flags = new Set(["--base", "--head", "--head-sha", "--labels", "--branch", "--reviews", "--checks", "--workflow-runs", "--root"]);
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--from-event") { o.fromEvent = true; continue; }
    if (!flags.has(a)) throw new UsageError(`unknown argument '${clean(a)}'`);
    const v = argv[++i];
    if (v === undefined) throw new UsageError(`${a} needs a value`);
    o[a.slice(2).replace(/-(.)/g, (_, c) => c.toUpperCase())] = v;
  }
  if (typeof o.labels === "string") o.labels = o.labels.split(",").map((s) => s.trim()).filter(Boolean);
  return o;
}

const readJsonFile = (f) => JSON.parse(fs.readFileSync(f, "utf8"));

async function main(argv) {
  if (argv[0] === "--self-test") {
    const r = selfTest();
    for (const l of r.lines) console.log(`  ok: ${l}`);
    for (const l of r.failures) console.log(`  SELFTEST FAIL: ${l}`);
    console.log(r.failures.length ? "SELFTEST: FAIL" : "SELFTEST: PASS");
    return r.failures.length ? 1 : 0;
  }
  if (argv[0] !== "check") throw new UsageError("usage: chain-touch.mjs check --base <ref> [--labels a,b] [--branch b] | check --from-event | --self-test");
  const o = parseArgs(argv.slice(1));
  const ctx = { root: o.root, head: o.head, headSha: o.headSha, labels: o.labels, branch: o.branch, base: o.base };
  if (o.reviews) ctx.reviews = readJsonFile(o.reviews);
  if (o.checks) ctx.checkRuns = readJsonFile(o.checks);
  if (o.workflowRuns) ctx.workflowRuns = readJsonFile(o.workflowRuns);
  if (o.fromEvent) {
    const ev = await eventContext(process.env);
    Object.assign(ctx, { branch: ev.branch, labels: ev.labels, headSha: ev.headSha, reviews: ev.reviews, checkRuns: ev.checkRuns, workflowRuns: ev.workflowRuns, base: o.base || ev.base });
  }
  if (!ctx.base) throw new UsageError("check needs --base <ref> (or --from-event)");
  const r = await runCheck(ctx);
  console.log(`lq / chain-touch: base ${r.base.slice(0, 12)} head ${r.head.slice(0, 12)} branch ${clean(ctx.branch || "(none)")} labels [${(ctx.labels || []).map(clean).join(", ")}]`);
  console.log(`  changed files: ${(r.files || []).length}; class C: ${r.classC.length ? r.classC.map((c) => clean(c.path)).join(", ") : "none"}`);
  for (const n of r.notes) console.log(`  note: ${clean(n)}`);
  if (r.ok) { console.log("PASS"); return 0; }
  console.log("REFUSED:");
  for (const e of r.errors) console.log(`  - ${clean(e)}`);
  return 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then((code) => process.exit(code), (e) => {
    if (e instanceof UsageError) { console.error(`chain-touch: ${clean(e.message)}`); process.exit(2); }
    console.error(`chain-touch: REFUSED (fail closed): ${clean(e.message)}`);
    process.exit(1);
  });
}
