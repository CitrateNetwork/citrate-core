#!/usr/bin/env node
// created: 2026-11-28 | branch: feat/v60-lq-wp07-chain-agreement
// author: Larry Klosowski (LQ lane, LQ-WP07; Claude Opus 5.5)
// status: active
//
// LQ-WP07 (planset 2026-11-07-v060, lanes/LQ_NON_REGRESSION_GUARD.md): r1007 block-hash agreement,
// the check `lq / chain-agreement` (.github/workflows/lq-chain-agreement.yml).
//
// Data source (Rule 7): scripts/e2e/cold-sync.py verdict.json files from a cold start of the pinned
// runtime-deps node asset against the live r1007 network (rpc.citrate.ai and the cut's bootnodes),
// bounded mode, anchored-prefix agreement (sync.by_number: the node's and the network's block hash
// at height 0, every K-th height and the anchor). Pins: release/components.json (citrate-node rev,
// via scripts/component-pins.mjs digest) and release/network-cut.json (consensus fingerprint).
//
//   node scripts/ci/lq/chain-agreement.mjs plan --root <dir> --arch x86_64|aarch64
//       prints asset=, sha256=, rev=, fingerprint= lines (shape-checked) for $GITHUB_OUTPUT
//   node scripts/ci/lq/chain-agreement.mjs wrong-genesis --root <dir> --profiles <file> [--name <p>] --out <dir>
//       writes wrong-profile.json and wrong-book.json (the profile with its genesis flipped)
//   node scripts/ci/lq/chain-agreement.mjs summarize --verdict <f> --negative <f> --arch <a> --rev <sha>
//        --expect-sha256 <hex> --expect-fingerprint <0x..> --out <agreement-<arch>.json>
//   node scripts/ci/lq/chain-agreement.mjs compare --out <file> <agreement-a.json> <agreement-b.json>...
// Exit 0 pass, 1 refused (nothing written), 2 usage.
//
// The r1007 identity (chain id and genesis) is a constant here, not read from the tree under test:
// the workflow runs the base branch's copy of this file, so a PR that moves the profile or the cut
// to another chain is refused by the rules it did not write. Honest label: a pass is "agreement to
// height N" on a bounded run, never "sync complete".
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
// Imported, never executed from the tree under test: the workflow runs this file and
// component-pins.mjs from the base branch, and only reads the head's pins as data.
import { component, parsePins, pinnedDigest, revFor } from "../../component-pins.mjs";

export const R1007 = Object.freeze({
  chainId: 40204,
  genesisHash: "0xcf68634502b4fca1e977ac8dd35aa006bb40c04208d5cd55f1c2b508bba96631",
});
export const MIN_SAMPLED = 3;
const ARCHES = new Set(["x86_64", "aarch64"]);
const HASH = /^0x[0-9a-f]{64}$/;
const FP = /^0x[0-9a-f]{32}$/;
const SHA = /^[0-9a-f]{64}$/;
const REV = /^[0-9a-f]{40}$/;
// cold-sync.py rules that must be ok on the positive run (the rest may be ok or skip, never FAIL).
const MUST_OK = ["profile", "node_binary", "endpoint_identity", "local_genesis", "node_chain_id", "fingerprint", "reaches_target", "agrees"];

export class Refused extends Error {}
class UsageError extends Error {}
const refuse = (msg) => { throw new Refused(msg); };
const readJson = (p) => JSON.parse(fs.readFileSync(p, "utf8"));
const statusOf = (v) => Object.fromEntries((Array.isArray(v?.rules) ? v.rules : []).map((r) => [r?.rule, r?.status]));

/** The pinned node asset, its sha256 pin and the fingerprint it must report, for one Linux arch. */
export function plan(root, arch) {
  if (!ARCHES.has(arch)) refuse(`arch ${String(arch)} has no runner here (x86_64 or aarch64)`);
  const triple = `${arch}-unknown-linux-gnu`;
  const comps = readJson(path.join(root, "release/components.json"));
  const node = component(comps, "citrate-node");
  const target = node.source?.target_revs?.[triple];
  const rev = String(revFor(node, triple) ?? "");
  if (!REV.test(rev)) refuse(`citrate-node rev for ${triple} is not a 40-hex commit: ${rev}`);
  const asset = `citrate-${triple}-${rev.slice(0, 12)}`;
  const named = node.artifacts?.[triple]?.runtime_deps_asset;
  if (named !== undefined && named !== asset) refuse(`components.json names ${named} for ${triple}, the rev gives ${asset}`);
  const pinsFile = path.join(root, "src-tauri/runtime-deps.sha256");
  const pins = parsePins(fs.existsSync(pinsFile) ? fs.readFileSync(pinsFile, "utf8") : "");
  let sha256;
  try { sha256 = pinnedDigest(comps, pins, "citrate-node", triple); } catch (e) { refuse(String(e.message)); }
  if (!SHA.test(sha256)) refuse(`no sha256 pin for citrate-node ${triple}`);
  // An asset the runtime-deps manifest also pins must carry the same digest there.
  const listed = pins.get(asset) || [];
  if (listed.length && (listed.length !== 1 || listed[0] !== sha256)) refuse(`src-tauri/runtime-deps.sha256 pins ${asset} as ${listed.join(", ")}, components.json as ${sha256}`);
  const cut = readJson(path.join(root, "release/network-cut.json"));
  if (!FP.test(cut.consensusFingerprint ?? "")) refuse("release/network-cut.json has no consensusFingerprint");
  let fingerprint;
  if (rev === cut.chainRev) fingerprint = cut.consensusFingerprint;
  else if (target && target.baseEquivalentFingerprint === cut.consensusFingerprint && FP.test(target.consensusFingerprint ?? "")) fingerprint = target.consensusFingerprint;
  else refuse(`citrate-node ${rev.slice(0, 12)} for ${triple} is not the cut's rev ${String(cut.chainRev).slice(0, 12)} and records no consensusFingerprint with a baseEquivalentFingerprint equal to the cut's (LQ-WP06)`);
  return { arch, triple, rev, asset, sha256, fingerprint };
}

/** The same profile and address book with another genesis (last hex digit flipped). */
export function wrongGenesis(root, profilesPath, name, out) {
  let doc = readJson(profilesPath);
  if (doc.profiles && typeof doc.profiles === "object") doc = doc.profiles[name || doc.active] || refuse(`no profile ${name || doc.active}`);
  const g = String(doc.genesisHash || "").toLowerCase();
  if (!HASH.test(g)) refuse("profile genesisHash is not 0x + 64 hex");
  doc = { ...doc, genesisHash: g.slice(0, -1) + (g.at(-1) !== "0" ? "0" : "1") };
  const book = { ...readJson(path.join(root, "src-tauri", doc.addressBook)), genesisHash: doc.genesisHash };
  fs.mkdirSync(out, { recursive: true });
  fs.writeFileSync(path.join(out, "wrong-profile.json"), JSON.stringify(doc));
  fs.writeFileSync(path.join(out, "wrong-book.json"), JSON.stringify(book));
}

/** One arch's evidence record from the positive and wrong-genesis verdicts. */
export function summarize(v, neg, o) {
  if (!ARCHES.has(o.arch)) refuse(`arch ${String(o.arch)} is not x86_64 or aarch64`);
  if (!REV.test(o.rev ?? "")) refuse("--rev must be the 40-hex node rev");
  if (!SHA.test(o.expectSha256 ?? "")) refuse("--expect-sha256 must be 64 hex");
  if (!FP.test(o.expectFingerprint ?? "")) refuse("--expect-fingerprint must be 0x + 32 hex");
  const st = statusOf(v);
  const bad = MUST_OK.filter((r) => st[r] !== "ok").concat(Object.keys(st).filter((r) => st[r] === "FAIL"));
  if (v?.verdict !== "PASS" || bad.length) refuse(`the cold-sync verdict is not PASS (${v?.verdict}; rules not ok: ${[...new Set(bad)].join(", ") || "none listed"})`);
  if (v.profile?.chainId !== R1007.chainId) refuse(`chain id ${v.profile?.chainId} is not r1007's ${R1007.chainId}`);
  if (v.profile?.genesisHash !== R1007.genesisHash || v.node?.local_genesis !== R1007.genesisHash)
    refuse(`block 0 ${v.node?.local_genesis} (profile ${v.profile?.genesisHash}) is not the r1007 genesis ${R1007.genesisHash}`);
  if (v.node?.sha256 !== o.expectSha256) refuse(`node bytes sha256 ${v.node?.sha256} are not the pinned ${o.expectSha256} (a dirty or other binary)`);
  if (v.node?.consensus_fingerprint !== o.expectFingerprint) refuse(`node consensus fingerprint ${v.node?.consensus_fingerprint} is not the expected ${o.expectFingerprint}`);
  const by = v.sync?.by_number;
  if (!Array.isArray(by) || by.length < MIN_SAMPLED) refuse(`fewer than ${MIN_SAMPLED} sampled heights (${Array.isArray(by) ? by.length : "no by_number"}); run bounded with --every below the target`);
  if ((v.sync.mismatches || []).length) refuse(`cold-sync recorded a mismatch at height ${v.sync.mismatches[0]?.height}`);
  let prev = -1;
  for (const s of by) {
    if (!Number.isInteger(s?.height) || s.height <= prev) refuse(`sampled heights are not strictly increasing at ${s?.height}`);
    if (!HASH.test(s.node ?? "") || s.node !== s.network) refuse(`height ${s.height}: node ${s.node}, network ${s.network}`);
    prev = s.height;
  }
  if (by[0].height !== 0 || by[0].node !== R1007.genesisHash) refuse("the first sampled height is not block 0 at the r1007 genesis");
  const anchor = v.sync.anchor;
  if (!anchor || anchor.height !== prev || anchor.hash !== by.at(-1).node) refuse("the last sampled height is not the agreement anchor");
  const ns = statusOf(neg);
  if (neg?.verdict !== "FAIL" || ns.endpoint_identity !== "FAIL" || ns.local_genesis !== "FAIL" || ns.reaches_target !== "skip")
    refuse("the wrong genesis case was not refused by both the endpoint and the node");
  return {
    version: 1, tool: "lq chain-agreement", arch: o.arch, rev: o.rev, chainId: R1007.chainId, genesisHash: R1007.genesisHash,
    consensusFingerprint: o.expectFingerprint, node_sha256: o.expectSha256, height: anchor.height,
    label: `agreement to height ${anchor.height}`, sampled: by.map((s) => ({ height: s.height, hash: s.node })),
    negative: "wrong genesis refused (endpoint and node)", ...(o.run ? { run: o.run } : {}),
  };
}

/** Cross-arch: every arch reports the same block hash at every common sampled height. */
export function compare(list) {
  const arches = new Set(list.map((s) => s?.arch));
  if (list.length < 2 || arches.size !== list.length) refuse(`needs at least two arches, each once (got ${list.map((s) => s?.arch).join(", ")})`);
  for (const s of list) {
    if (s?.version !== 1 || !Array.isArray(s.sampled) || !s.sampled.every((x) => Number.isInteger(x?.height) && HASH.test(x?.hash ?? "")))
      refuse(`${s?.arch}: not an agreement summary (version 1 with sampled heights)`);
    for (const k of ["rev", "chainId", "genesisHash", "consensusFingerprint"])
      if (s[k] !== list[0][k]) refuse(`${s.arch} ${k} ${s[k]} differs from ${list[0].arch}'s ${list[0][k]}`);
  }
  const maps = list.map((s) => new Map(s.sampled.map((x) => [x.height, x.hash])));
  const common = [...maps[0].keys()].filter((ht) => maps.every((m) => m.has(ht))).sort((a, b) => a - b);
  if (common.length < MIN_SAMPLED) refuse(`fewer than ${MIN_SAMPLED} common sampled heights (${common.join(", ")})`);
  for (const ht of common) {
    const hs = maps.map((m) => m.get(ht));
    if (new Set(hs).size !== 1) refuse(`height ${ht}: ${list.map((s, i) => `${s.arch} ${hs[i]}`).join(", ")}`);
  }
  const names = list.map((s) => s.arch).join(" and ");
  return {
    version: 1, tool: "lq chain-agreement compare", rev: list[0].rev, arches: list.map((s) => s.arch),
    heights: Object.fromEntries(list.map((s) => [s.arch, s.height])), common_heights: common,
    label: `${names} report the same block hash at ${common.length} common sampled heights (agreement to height ${Math.min(...list.map((s) => s.height))})`,
  };
}

function args(argv) {
  const o = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i].startsWith("--")) {
      if (i + 1 >= argv.length) throw new UsageError(`${argv[i]} needs a value`);
      o[argv[i].slice(2)] = argv[++i];
    } else o._.push(argv[i]);
  }
  return o;
}

function main(argv) {
  const [cmd, ...rest] = argv;
  const o = args(rest);
  const write = (file, doc) => { fs.writeFileSync(file, JSON.stringify(doc, null, 2) + "\n"); console.log(doc.label ?? file); };
  if (cmd === "plan") {
    const p = plan(path.resolve(o.root || "."), o.arch);
    for (const k of ["asset", "sha256", "rev", "fingerprint"]) console.log(`${k}=${p[k]}`);
  } else if (cmd === "wrong-genesis") {
    if (!o.profiles || !o.out) throw new UsageError("wrong-genesis needs --profiles and --out");
    wrongGenesis(path.resolve(o.root || "."), o.profiles, o.name || null, o.out);
  } else if (cmd === "summarize") {
    if (!o.verdict || !o.out) throw new UsageError("summarize needs --verdict and --out");
    const neg = o.negative && fs.existsSync(o.negative) ? readJson(o.negative) : null;
    write(o.out, summarize(readJson(o.verdict), neg, { arch: o.arch, rev: o.rev, expectSha256: o["expect-sha256"], expectFingerprint: o["expect-fingerprint"], run: o.run }));
  } else if (cmd === "compare") {
    if (!o.out || o._.length === 0) throw new UsageError("compare needs --out and the summaries");
    write(o.out, compare(o._.map(readJson)));
  } else throw new UsageError("usage: chain-agreement.mjs plan|wrong-genesis|summarize|compare ... (see the header)");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    const usage = e instanceof UsageError;
    // Text from the verdicts is data: strip control characters so it can never form a workflow command.
    const msg = String(e?.message ?? e).replace(/[\u0000-\u001f\u007f]/g, " ");
    console.error(`::error title=lq / chain-agreement::${usage ? "usage: " : "refused: "}${msg}`);
    process.exit(usage ? 2 : 1);
  }
}
