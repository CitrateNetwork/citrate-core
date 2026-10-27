// @vitest-environment node
//
// RC52 gate G1 (g1-macos), WP-H-MACOS: unit tests of the macOS packaged acceptance harness
// (scripts/ci/macos-acceptance.mjs). They test the harness logic only: the test-machine marker,
// the machine-safety guard and the guarded runner, the parsers (ps, lsof, codesign, spctl, stapler,
// hdiutil, security), the Mach-O code digest, the bundle spec from release/components.json, the
// data-location rules, the verdict and the self-test judge. Nothing here mounts a DMG, installs or
// launches anything, or touches a keychain: `main` runs with fake dependencies, and the one real
// CLI run below is refused before it runs any command.
//
// Tripwires: the harness refuses on a machine without the test-machine marker (CLI run); every
// keychain service and data root the app code names is documented; the bundle spec follows the
// overlay release.yml builds with; process control lives only in realDeps; the workflow runs only
// on macOS runners and never interpolates a dispatch input into a script.
import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import * as H from "./macos-acceptance.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "..", "..");
const read = (p) => { try { return fs.readFileSync(p, "utf8"); } catch { return ""; } };
const SHA = "a".repeat(64);
const INSTALL = "/var/folders/xy/T/citrate-macos-acc-1/run.2/Applications/Citrate Core.app";

describe("arguments", () => {
  it("needs a DMG and its sha256 unless self-testing", () => {
    expect(() => H.parseArgs([])).toThrow(H.UsageError);
    expect(() => H.parseArgs(["--dmg", "x.dmg"])).toThrow(/sha256/);
    expect(() => H.parseArgs(["--dmg", "x.dmg", "--sha256", "abc"])).toThrow(/sha256/);
    const o = H.parseArgs(["--dmg", "x.dmg", "--sha256", SHA.toUpperCase()]);
    expect(o.sha256).toBe(SHA);
    expect(o.unsignedSmoke).toBe(false);
    expect(o.quitMethod).toBe("apple-event");
    expect(H.parseArgs(["--self-test"]).selfTest).toBe(true);
  });
  it("defaults to the inspect dry run; install and launch need --mode full", () => {
    expect(H.parseArgs(["--dmg", "x", "--sha256", SHA]).mode).toBe("inspect");
    expect(H.parseArgs(["--dmg", "x", "--sha256", SHA, "--mode", "verify"]).mode).toBe("verify");
    expect(H.parseArgs(["--dmg", "x", "--sha256", SHA, "--mode", "full"]).mode).toBe("full");
    expect(() => H.parseArgs(["--dmg", "x", "--sha256", SHA, "--mode", "install"])).toThrow(/--mode must be/);
  });
  it("has no flag that bypasses the test-machine marker or weakens a release run", () => {
    for (const f of ["--test-machine", "--verify-only", "--unsigned-rc-smoke", "--no-rpc-check"]) {
      expect(() => H.parseArgs(["--dmg", "x", "--sha256", SHA, f]), f).toThrow(/unknown/);
    }
    expect(() => H.parseArgs(["--dmg", "x", "--sha256", SHA, "--quit-method", "term"])).toThrow(/unknown/);
  });
  it("rejects unknown flags and flags without values", () => {
    expect(() => H.parseArgs(["--dmg", "x", "--sha256", SHA, "--yolo"])).toThrow(/unknown/);
    expect(() => H.parseArgs(["--dmg", "--sha256", SHA])).toThrow(/needs a value/);
  });
});

// ------------------------------------------------------------------ test-machine marker
const hostsFile = (text, extra = {}) => ({ uid: 501, mode: 0o100644, text, ...extra });
const marker = (patch = {}) => ({
  platform: "darwin", env: { [H.MARKER_ENV]: "1" }, hostname: "acc-mini.local", uid: 501,
  hostsFile: hostsFile("# test machines\nacc-mini\n"), ...patch,
});

describe("test-machine marker", () => {
  it("passes with the env marker and this host listed in an owned, private hosts file", () => {
    expect(H.evaluateMarker(marker())).toEqual({ ok: true, refusals: [] });
    expect(H.evaluateMarker(marker({ hostname: "ACC-MINI" })).ok).toBe(true);
    expect(H.hostsFilePath("/Users/t")).toBe("/Users/t/.config/citrate/macos-acceptance-test-hosts");
  });
  it.each([
    ["no env marker", { env: {} }, /CITRATE_MACOS_ACCEPTANCE_TEST_MACHINE=1 is not set/],
    ["a marker other than 1", { env: { [H.MARKER_ENV]: "true" } }, /is not set/],
    ["no hosts file", { hostsFile: null }, /no test-machine hosts file/],
    ["this host not listed", { hostname: "release-mac" }, /release-mac\) is not listed/],
    ["a commented-out host", { hostsFile: hostsFile("# acc-mini\n") }, /not listed/],
    ["a hosts file owned by someone else", { hostsFile: hostsFile("acc-mini\n", { uid: 0 }) }, /not owned by you/],
    ["a group-writable hosts file", { hostsFile: hostsFile("acc-mini\n", { mode: 0o100664 }) }, /group- or world-writable/],
    ["a world-writable hosts file", { hostsFile: hostsFile("acc-mini\n", { mode: 0o100646 }) }, /group- or world-writable/],
    ["Linux", { platform: "linux" }, /not macOS/],
    ["an empty hostname", { hostname: "" }, /not listed/],
  ])("refuses with %s", (_n, patch, re) => {
    const m = H.evaluateMarker(marker(patch));
    expect(m.ok).toBe(false);
    expect(m.refusals.join("\n")).toMatch(re);
  });
});

// ------------------------------------------------------------------ processes and listeners
describe("process parsing and kill safety", () => {
  const PS = [
    "    1     0 /sbin/launchd",
    `  500     1 ${INSTALL}/Contents/MacOS/Citrate Core`,
    `  501   500 ${INSTALL}/Contents/MacOS/citrate`,
    "  502   501 /bin/sh",
    `  503   500 ${INSTALL}/Contents/MacOS/ipfs`,
    "  600     1 /usr/sbin/cfprefsd",
    "",
  ].join("\n");
  it("parses paths with spaces and walks the whole subtree", () => {
    const p = H.parsePs(PS);
    expect(p).toHaveLength(6);
    expect(p[1]).toEqual({ pid: 500, ppid: 1, path: `${INSTALL}/Contents/MacOS/Citrate Core` });
    expect([...H.descendants(p, 500)].sort()).toEqual([501, 502, 503]);
    expect(H.descendants(p, 999).size).toBe(0);
  });
  it("inside means inside, not a sibling with the same prefix", () => {
    expect(H.isInside(`${INSTALL}/Contents/MacOS/citrate`, INSTALL)).toBe(true);
    expect(H.isInside(`${INSTALL}2/Contents/MacOS/citrate`, INSTALL)).toBe(false);
    expect(H.isInside(INSTALL, INSTALL)).toBe(false);
  });
  it("signals only the run's own processes, never a reused pid", () => {
    const own = new Map([[502, "/bin/sh"]]);
    expect(H.mayKill({ pid: 501, path: `${INSTALL}/Contents/MacOS/citrate` }, INSTALL, own)).toBe(true);
    expect(H.mayKill({ pid: 502, path: "/bin/sh" }, INSTALL, own)).toBe(true);
    expect(H.mayKill({ pid: 502, path: "/usr/bin/ssh" }, INSTALL, own)).toBe(false); // pid reused
    expect(H.mayKill({ pid: 600, path: "/usr/sbin/cfprefsd" }, INSTALL, own)).toBe(false);
    expect(H.mayKill({ pid: 1, path: `${INSTALL}/x` }, INSTALL, own)).toBe(false);
    expect(H.mayKill(undefined, INSTALL, own)).toBe(false);
  });
});

describe("listeners", () => {
  const policy = H.parseListenerPolicy(read(path.join(here, "macos-acceptance", "listener-policy.txt")));
  const P = (exe) => `${INSTALL}/Contents/MacOS/${exe}`;
  const paths = new Map([[501, P("citrate")], [700, P("ipfs")], [702, P("mem-mcp")], [704, P("llama-server")]]);
  const lsof = (rows) => rows.map(([pid, cmd, socks]) => [`p${pid}`, `c${cmd}`, ...socks.flatMap(([proto, n], i) => [`f${i + 3}`, `P${proto}`, `n${n}`])]).flat().join("\n") + "\n";
  // The 0.5.1 app's sockets as the core#558 review read them with lsof (read-only, nothing launched),
  // LAN addresses replaced by documentation-style private ones. ipfs: swarm on 4001, six mDNS
  // sockets on *:5353, two unconnected wildcard UDP on random ports, per-interface mDNS sockets
  // (one LAN v4, nine link-local v6 in lsof's embedded-scope form, one unique-local v6).
  const REVIEWED = lsof([
    [501, "citrate", [["TCP", "127.0.0.1:8545"], ["TCP", "127.0.0.1:8546"], ["TCP", "*:30303"], ["UDP", "192.168.1.23:55012->1.1.1.1:53"]]],
    [700, "ipfs", [
      ["TCP", "*:4001"], ["TCP", "*:4001"], ["TCP", "127.0.0.1:5001"], ["TCP", "127.0.0.1:48080"],
      ["UDP", "*:4001"], ["UDP", "*:4001"],
      ...Array.from({ length: 6 }, () => ["UDP", "*:5353"]),
      ["UDP", "*:61234"], ["UDP", "*:58211"],
      ["UDP", "192.168.1.23:63001"],
      ...Array.from({ length: 9 }, (_, i) => ["UDP", `[fe80:${(i + 4).toString(16)}::1c2b:3aff:fe11:22${10 + i}]:${64000 + i}`]),
      ["UDP", "[fd7a:115c:a1e0::1]:62001"],
    ]],
    [702, "mem-mcp", [["TCP", "127.0.0.1:7071"]]],
  ]);
  // The same sidecar read on the MAC-lane machine 2026-10-27 (no per-interface sockets at that moment).
  const LIVE = lsof([[700, "ipfs", [["TCP", "*:4001"], ["TCP", "*:4001"], ["TCP", "127.0.0.1:5001"], ["TCP", "127.0.0.1:48080"],
    ["UDP", "*:4001"], ["UDP", "*:4001"], ["UDP", "*:5353"], ["UDP", "*:5353"], ["UDP", "*:5353"], ["UDP", "*:5353"]]]]);
  const evalL = (text, own = []) => H.evaluateListeners(H.parseLsof(text), paths, policy, own);

  it("parses lsof -F output and keeps the remote side of a connected socket", () => {
    const l = H.parseLsof("p501\nccitrate\nf7\nPTCP\nn127.0.0.1:8545\nf8\nPUDP\nn127.0.0.1:5353->224.0.0.251:5353\n");
    expect(l).toEqual([
      { pid: 501, command: "citrate", proto: "tcp", local: "127.0.0.1:8545", remote: "" },
      { pid: 501, command: "citrate", proto: "udp", local: "127.0.0.1:5353", remote: "224.0.0.251:5353" },
    ]);
  });
  it("classifies loopback, wildcard and interface addresses (lsof's embedded-scope link-local too)", () => {
    expect(H.classifyAddress("127.0.0.1:8545")).toBe("loopback");
    expect(H.classifyAddress("[::1]:5001")).toBe("loopback");
    expect(H.classifyAddress("*:30303")).toBe("wildcard");
    expect(H.classifyAddress("[::]:1")).toBe("wildcard");
    expect(H.splitHostPort("[fe80:4::1]:64000")).toEqual({ host: "fe80:4::1", port: 64000 });
    expect(H.normalizeAddr("[fe80:4::1C2B]")).toBe("fe80::1c2b");
    expect(H.normalizeAddr("fe80::1%en0")).toBe("fe80::1");
    for (const a of ["192.168.1.23", "10.0.0.5", "172.20.1.1", "169.254.3.4", "fe80:4::1", "fd7a:115c:a1e0::1"]) expect(H.isInterfaceAddress(a), a).toBe(true);
    for (const a of ["8.8.8.8", "172.32.0.1", "2001:db8::5", "100.64.0.1"]) expect(H.isInterfaceAddress(a), a).toBe(false);
    expect(H.isInterfaceAddress("[2001:db8::5]", ["2001:db8::5"])).toBe(true); // this machine's own global v6
  });
  it("the policy file parses, and refuses a TCP 'any port' rule or an unknown scope", () => {
    expect(policy.map((r) => `${r.exe} ${r.proto} ${r.port} ${r.bind}`)).toEqual([
      "citrate tcp 30303 wildcard", "ipfs tcp 4001 wildcard", "ipfs udp 4001 wildcard", "ipfs udp 5353 wildcard",
      "ipfs udp ephemeral interface", "ipfs udp ephemeral wildcard",
    ]);
    expect(() => H.parseListenerPolicy("ipfs tcp ephemeral wildcard any\n")).toThrow(/udp only/);
    expect(() => H.parseListenerPolicy("ipfs udp 5353 everywhere x\n")).toThrow(/bad bind/);
    expect(() => H.parseListenerPolicy("ipfs udp * wildcard x\n")).toThrow(/bad port/);
    expect(() => H.parseListenerPolicy("ipfs sctp 1 wildcard x\n")).toThrow(/bad proto/);
  });
  it("passes the 0.5.1 app's real sockets (review of core#558) and the live read", () => {
    const r = evalL(REVIEWED);
    expect(r.bad).toEqual([]);
    expect(r.ok).toBe(true);
    expect(r.outbound.map((o) => o.local)).toEqual(["192.168.1.23:55012"]);
    expect(r.allowed.filter((a) => a.local === "*:5353")).toHaveLength(6);
    expect(r.allowed.filter((a) => a.kind === "interface")).toHaveLength(11);
    expect(r.loopback).toHaveLength(5);
    expect(evalL(LIVE).ok).toBe(true);
  });
  it.each([
    ["a non-ipfs sidecar on UDP 5353", [702, "mem-mcp", [["UDP", "*:5353"]]], "mem-mcp udp *:5353"],
    ["an unexpected TCP listener on all interfaces", [700, "ipfs", [["TCP", "*:5002"]]], "ipfs tcp *:5002"],
    ["ipfs TCP on an ephemeral port on an interface", [700, "ipfs", [["TCP", "192.168.1.23:63001"]]], "ipfs tcp 192.168.1.23:63001"],
    ["ipfs UDP below the ephemeral range on an interface", [700, "ipfs", [["UDP", "192.168.1.23:5000"]]], "ipfs udp 192.168.1.23:5000"],
    ["ipfs UDP below the ephemeral range on all interfaces", [700, "ipfs", [["UDP", "*:8000"]]], "ipfs udp *:8000"],
    ["ipfs UDP on a public address", [700, "ipfs", [["UDP", "8.8.4.4:63001"]]], "ipfs udp 8.8.4.4:63001"],
    ["ipfs 5353 on one interface (only the wildcard bind is documented)", [700, "ipfs", [["UDP", "192.168.1.23:5353"]]], "ipfs udp 192.168.1.23:5353"],
    ["the node's ephemeral UDP on all interfaces", [501, "citrate", [["UDP", "*:61234"]]], "citrate udp *:61234"],
    ["the node's P2P port held by llama-server", [704, "llama-server", [["TCP", "*:30303"]]], "llama-server tcp *:30303"],
  ])("fails %s, even next to the real sockets", (_n, row, want) => {
    const r = evalL(REVIEWED + lsof([row]));
    expect(r.ok).toBe(false);
    expect(r.bad.map((b) => `${b.exe} ${b.proto} ${b.local}`)).toEqual([want]);
  });
});

// ------------------------------------------------------------------ signing output
describe("signing, Gatekeeper, notarization and image output", () => {
  const RELEASE = `Executable=/Volumes/Citrate Core/Citrate Core.app/Contents/MacOS/Citrate Core
Identifier=ai.citrate.core
Format=app bundle with Mach-O thin (arm64)
CodeDirectory v=20500 size=1234 flags=0x10000(runtime) hashes=27+7 location=embedded
Signature size=9012
Authority=Developer ID Application: Example Inc. (ABCDE12345)
Authority=Developer ID Certification Authority
Authority=Apple Root CA
Timestamp=27 Oct 2026 at 10:00:00
Notarization Ticket=stapled
TeamIdentifier=ABCDE12345
`;
  const ADHOC = `Executable=/x/Citrate Core.app/Contents/MacOS/Citrate Core
Identifier=ai.citrate.core
CodeDirectory v=20400 size=500 flags=0x2(adhoc) hashes=10+3 location=embedded
Signature=adhoc
TeamIdentifier=not set
`;
  it("reads a Developer ID release signature", () => {
    const s = H.parseCodesignDisplay(RELEASE);
    expect(s).toMatchObject({ identifier: "ai.citrate.core", teamId: "ABCDE12345", runtime: true, adhoc: false });
    expect(H.evaluateDeveloperId(s, "").ok).toBe(true);
    expect(H.evaluateDeveloperId(s, "ABCDE12345").ok).toBe(true);
    expect(H.evaluateDeveloperId(s, "ZZZZZ99999").why.join()).toMatch(/expected ZZZZZ99999/);
  });
  it("rejects ad hoc, no-team and no-hardened-runtime signatures", () => {
    const r = H.evaluateDeveloperId(H.parseCodesignDisplay(ADHOC), "");
    expect(r.ok).toBe(false);
    expect(r.why).toEqual(["ad hoc signature", "no Developer ID Application authority", "no team identifier", "hardened runtime flag missing"]);
    const noRuntime = H.parseCodesignDisplay(RELEASE.replace("flags=0x10000(runtime)", "flags=0x0(none)"));
    expect(H.evaluateDeveloperId(noRuntime, "").why).toEqual(["hardened runtime flag missing"]);
  });
  it("Gatekeeper passes only an accepted, notarized assessment", () => {
    const acc = H.parseSpctl("/t/Citrate Core.app: accepted\nsource=Notarized Developer ID\norigin=Developer ID Application: Example Inc. (ABCDE12345)\n", 0);
    expect(acc).toEqual({ accepted: true, source: "Notarized Developer ID" });
    expect(H.evaluateSpctl(acc)).toBe(true);
    expect(H.evaluateSpctl(H.parseSpctl("/t/a.app: accepted\nsource=Unnotarized Developer ID\n", 0))).toBe(false);
    expect(H.evaluateSpctl(H.parseSpctl("/t/a.app: rejected\nsource=no usable signature\n", 3))).toBe(false);
    expect(H.evaluateSpctl(H.parseSpctl("/t/a.app: accepted\nsource=Notarized Developer ID\n", 3))).toBe(false);
  });
  it("reads the stapler result", () => {
    expect(H.parseStapler("Processing: /t/Citrate Core.app\nThe validate action worked!\n", 0)).toBe(true);
    expect(H.parseStapler("Processing: /t/a.app\na.app does not have a ticket stapled to it.\n", 65)).toBe(false);
    expect(H.parseStapler("The validate action worked!\n", 1)).toBe(false);
  });
  it("reads hdiutil imageinfo and the Developer ID identities of find-identity", () => {
    expect(H.parseImageInfo("Format Description: UDIF read-only compressed (zlib)\nClass Name: CUDIFDiskImage\nChecksum Type: CRC32\nFormat: UDZO\n"))
      .toEqual({ format: "UDZO", checksumType: "CRC32" });
    const fi = `  1) 0123456789ABCDEF0123456789ABCDEF01234567 "Apple Development: t@example.com (XYZ)"
  2) 89ABCDEF0123456789ABCDEF0123456789ABCDEF "Developer ID Application: Example Inc. (ABCDE12345)"
     2 valid identities found
`;
    expect(H.developerIdIdentities(fi)).toEqual(["Developer ID Application: Example Inc. (ABCDE12345)"]);
    expect(H.developerIdIdentities("     0 valid identities found\n")).toEqual([]);
  });
});

// ------------------------------------------------------------------ Mach-O code digest
// Minimal synthetic 64-bit Mach-O: header, LC_SEGMENT_64 __LINKEDIT, LC_CODE_SIGNATURE, code,
// signature blob. Only the parts the digest reads are filled in.
function macho({ code = Buffer.from("code-bytes-0123456789"), sig = Buffer.from("SIGNATURE-A"), vmsize = 0x4000, flags = 0, withSig = true } = {}) {
  const ncmds = withSig ? 2 : 1, sizeofcmds = 72 + (withSig ? 16 : 0);
  const head = Buffer.alloc(32 + sizeofcmds);
  head.writeUInt32LE(0xfeedfacf, 0); head.writeUInt32LE(0x0100000c, 4); head.writeUInt32LE(2, 12);
  head.writeUInt32LE(ncmds, 16); head.writeUInt32LE(sizeofcmds, 20); head.writeUInt32LE(flags, 24);
  head.writeUInt32LE(0x19, 32); head.writeUInt32LE(72, 36); head.write("__LINKEDIT", 40, "latin1");
  const sigOff = head.length + code.length;
  head.writeBigUInt64LE(BigInt(vmsize), 32 + 32);
  head.writeBigUInt64LE(BigInt(code.length + sig.length), 32 + 48);
  if (withSig) { head.writeUInt32LE(0x1d, 104); head.writeUInt32LE(16, 108); head.writeUInt32LE(sigOff, 112); head.writeUInt32LE(sig.length, 116); }
  return Buffer.concat([head, code, withSig ? sig : Buffer.alloc(0)]);
}
function fat(slices) {
  const head = Buffer.alloc(8 + slices.length * 20);
  head.writeUInt32BE(0xcafebabe, 0); head.writeUInt32BE(slices.length, 4);
  let off = 4096;
  const body = [];
  slices.forEach((s, i) => {
    head.writeUInt32BE(s.cpu, 8 + i * 20); head.writeUInt32BE(0, 12 + i * 20);
    head.writeUInt32BE(off, 16 + i * 20); head.writeUInt32BE(s.buf.length, 20 + i * 20); head.writeUInt32BE(12, 24 + i * 20);
    body.push({ off, buf: s.buf });
    off += s.buf.length + 4096;
  });
  const out = Buffer.alloc(off);
  head.copy(out, 0);
  for (const b of body) b.buf.copy(out, b.off);
  return out;
}

describe("Mach-O code digest", () => {
  it("is the same for every signature of the same code (what Tauri's re-signing changes)", () => {
    const a = macho();
    const b = macho({ sig: Buffer.from("A-MUCH-LONGER-DEVELOPER-ID-SIGNATURE-WITH-CERTS"), vmsize: 0x8000 });
    expect(createHash("sha256").update(a).digest("hex")).not.toBe(createHash("sha256").update(b).digest("hex"));
    expect(H.machoCodeDigest(a)).toBe(H.machoCodeDigest(b));
    expect(H.machoCodeDigest(a)).toMatch(/^[0-9a-f]{64}$/);
  });
  it("changes when one code byte or a header field outside the signature sizes changes", () => {
    const base = H.machoCodeDigest(macho());
    expect(H.machoCodeDigest(macho({ code: Buffer.from("code-bytes-0123456788") }))).not.toBe(base);
    expect(H.machoCodeDigest(macho({ flags: 0x200000 }))).not.toBe(base);
  });
  it("fails closed on unsigned, foreign, truncated or out-of-range input", () => {
    expect(() => H.machoCodeDigest(macho({ withSig: false }))).toThrow(/no code signature/);
    expect(() => H.machoCodeDigest(Buffer.from("#!/bin/bash\necho hi\n"))).toThrow(/not a 64-bit/);
    expect(() => H.machoCodeDigest(macho().subarray(0, 40))).toThrow(/past the end/);
    const bad = macho(); bad.writeUInt32LE(bad.length + 10, 112);
    expect(() => H.machoCodeDigest(bad)).toThrow(/out of range/);
    const badCmd = macho(); badCmd.writeUInt32LE(4, 36);
    expect(() => H.machoCodeDigest(badCmd)).toThrow(/bad load command size/);
  });
  it("digests universal binaries slice by slice, wherever the slices sit", () => {
    const x = macho({ code: Buffer.from("x86-code") }), y = macho({ code: Buffer.from("arm-code") });
    const one = fat([{ cpu: 0x01000007, buf: x }, { cpu: 0x0100000c, buf: y }]);
    const resigned = fat([{ cpu: 0x01000007, buf: macho({ code: Buffer.from("x86-code"), sig: Buffer.from("OTHER-SIGNATURE-BLOB") }) }, { cpu: 0x0100000c, buf: y }]);
    expect(H.machoCodeDigest(one)).toBe(H.machoCodeDigest(resigned));
    const changed = fat([{ cpu: 0x01000007, buf: x }, { cpu: 0x0100000c, buf: macho({ code: Buffer.from("arm-cod3") }) }]);
    expect(H.machoCodeDigest(changed)).not.toBe(H.machoCodeDigest(one));
    const broken = Buffer.from(one); broken.writeUInt32BE(one.length, 16);
    expect(() => H.machoCodeDigest(broken)).toThrow(/past the end/);
  });
});

// ------------------------------------------------------------------ bundle spec and digests
describe("bundle spec from the release pins", () => {
  it("lists the release overlay's sidecars and a pin for each component on aarch64-apple-darwin", () => {
    const spec = H.loadReleaseSpec(repo);
    const manifest = JSON.parse(read(path.join(repo, "release", "components.json")));
    expect(spec.errors).toEqual([]);
    expect(spec.release).toBe(manifest.release);
    expect(spec.sidecars.map((s) => s.bin).sort()).toEqual([...spec.bins].sort());
    for (const s of spec.sidecars) expect(s.sha256, s.id).toMatch(/^[0-9a-f]{64}$/);
    expect(spec.bins).toContain("citrate");
  });
  it("records an unpinned component as an error instead of skipping it", () => {
    const manifest = { release: "9.9.9", components: [{ id: "x", external_bin: "binaries/x", artifacts: {} }] };
    const s = H.releaseBundleSpec({ manifest, pinsText: "", overlay: { bundle: { externalBin: ["binaries/x"] } } });
    expect(s.sidecars).toEqual([]);
    expect(s.errors.join()).toMatch(/no pinned artifact for 'x'/);
  });
  it("Contents/MacOS holds exactly the main executable and the listed sidecars", () => {
    const ok = { macosEntries: ["Citrate Core", "citrate", "ipfs"], exe: "Citrate Core", bins: ["citrate", "ipfs"], hasCodeResources: true, hasResources: true, requireSignature: true };
    expect(H.evaluateBundleStructure(ok).ok).toBe(true);
    const missing = H.evaluateBundleStructure({ ...ok, macosEntries: ["Citrate Core", "citrate"] });
    expect(missing.why.join()).toMatch(/missing in Contents\/MacOS: ipfs/);
    const extra = H.evaluateBundleStructure({ ...ok, macosEntries: [...ok.macosEntries, "helper"] });
    expect(extra.why.join()).toMatch(/not expected in Contents\/MacOS: helper/);
    expect(H.evaluateBundleStructure({ ...ok, hasCodeResources: false }).ok).toBe(false);
    expect(H.evaluateBundleStructure({ ...ok, hasCodeResources: false, requireSignature: false }).ok).toBe(true);
    expect(H.evaluateBundleStructure({ ...ok, hasResources: false }).ok).toBe(false);
  });
  it("compares each sidecar's code digest with its pinned bytes, failing closed", () => {
    const sc = [{ id: "citrate-node", bin: "citrate", sha256: "1".repeat(64) }, { id: "kubo", bin: "ipfs", sha256: "2".repeat(64) }];
    const pinned = new Map([["1".repeat(64), "d1"], ["2".repeat(64), "d2"]]);
    expect(H.evaluateSidecarDigests(sc, pinned, new Map([["citrate", "d1"], ["ipfs", "d2"]])).status).toBe("pass");
    const swapped = H.evaluateSidecarDigests(sc, pinned, new Map([["citrate", "d2"], ["ipfs", "d2"]]));
    expect(swapped.status).toBe("fail");
    expect(swapped.rows[0].why).toMatch(/!= pinned/);
    expect(H.evaluateSidecarDigests(sc, pinned, new Map([["citrate", "d1"]])).rows[1].why).toBe("not in the bundle");
    const noPin = H.evaluateSidecarDigests(sc, new Map([["1".repeat(64), "d1"]]), new Map([["citrate", "d1"], ["ipfs", "d2"]]));
    expect(noPin.status).toBe("error");
    expect(noPin.rows[1].why).toMatch(/not in --pinned-dir/);
    expect(H.evaluateSidecarDigests(sc, pinned, new Map([["citrate", new Error("no code signature")], ["ipfs", "d2"]])).status).toBe("fail");
    expect(H.evaluateSidecarDigests([], pinned, new Map()).status).toBe("error");
  });
});

// ------------------------------------------------------------------ data locations
describe("data locations", () => {
  const doc = H.parseLocations(read(path.join(here, "macos-acceptance", "data-locations.txt")));
  it("parses the documented list (paths with spaces, relative to HOME)", () => {
    expect(doc.map((d) => d.prefix)).toEqual(expect.arrayContaining([
      "Library/Application Support/ai.citrate.core", "Library/Caches/ai.citrate.core", "Library/Logs/ai.citrate.core",
      "Library/WebKit/ai.citrate.core", "Library/Preferences/ai.citrate.core.plist",
    ]));
    expect(() => H.parseLocations("/etc/passwd | absolute\n")).toThrow(/bad location/);
    expect(() => H.parseLocations("Library/../.. | escape\n")).toThrow(/bad location/);
  });
  it("Citrate's files pass only under a documented location of a given home", () => {
    const home = "/r/run.1/home";
    const r = H.evaluateLocations([
      `${home}/Library/Application Support/ai.citrate.core/node/db`,
      `${home}/Library/Application Support`,
      `${home}/Library/Preferences/ai.citrate.core.plist`,
      `${home}/Library/Preferences/ai.citrate.core.plist.lockfile`,
      `${home}/.citrate/wallet`,
      `${home}/.ipfs/config`,
      `${home}/Library/Caches/com.apple.WebKit.Networking/x`,
      "/private/tmp/citrate-node.sock",
      `${home}/Library/Application Support/ai.citrate.core-old/x`,
    ], [home], doc, ["citrate", "ipfs"]);
    expect(r.ok).toBe(false);
    expect(r.documented).toEqual([`${home}/Library/Application Support/ai.citrate.core/node/db`, `${home}/Library/Preferences/ai.citrate.core.plist`]);
    expect(r.undocumented).toEqual([
      `${home}/Library/Preferences/ai.citrate.core.plist.lockfile`, `${home}/.citrate/wallet`, `${home}/.ipfs/config`,
      "/private/tmp/citrate-node.sock", `${home}/Library/Application Support/ai.citrate.core-old/x`,
    ]);
    expect(r.unrelated).toEqual([`${home}/Library/Application Support`, `${home}/Library/Caches/com.apple.WebKit.Networking/x`]);
    expect(H.evaluateLocations([`${home}/Library/Logs/ai.citrate.core/app.log`], [home], doc, []).ok).toBe(true);
  });
  it("attributes by the part below the scanned root, so a run dir named citrate-* does not make every file Citrate's", () => {
    const home = "/w/_temp/citrate-macos-acc-1/run.2/home";
    const r = H.evaluateLocations([
      `${home}/.selftest-pids`, `${home}/Library`, `${home}/Library/Application Support`,
      `${home}/Library/Application Support/ai.citrate.core/x`, `${home}/Library/citrate-undocumented/x`,
    ], [home], doc, ["citrate", "ipfs"], [home]);
    expect(r.unrelated).toEqual([`${home}/.selftest-pids`, `${home}/Library`, `${home}/Library/Application Support`]);
    expect(r.documented).toEqual([`${home}/Library/Application Support/ai.citrate.core/x`]);
    expect(r.undocumented).toEqual([`${home}/Library/citrate-undocumented/x`]);
    // outside the run dir: relative to the scanned root that holds the path (the longest one)
    const o = H.evaluateLocations(["/private/tmp/citrate-node.sock", "/private/tmp/other", "/Users/citrate-tester/Library/Caches/x"],
      ["/Users/citrate-tester"], doc, ["citrate"], ["/private/tmp", "/Users/citrate-tester"]);
    expect(o.undocumented).toEqual(["/private/tmp/citrate-node.sock"]);
    expect(o.unrelated).toEqual(["/private/tmp/other", "/Users/citrate-tester/Library/Caches/x"]);
  });
  it("attributes by name: citrate anywhere, or a sidecar name or .ipfs as a path segment", () => {
    expect(H.isCitratePath("/u/.ipfs/config", [])).toBe(true);
    expect(H.isCitratePath("/tmp/hermes/x", ["hermes"])).toBe(true);
    expect(H.isCitratePath("/tmp/ipfsish", ["ipfs"])).toBe(false);
    expect(H.isCitratePath("/u/Library/Caches/com.apple.x", ["ipfs"])).toBe(false);
  });
  it("the walk finds new files, skips excluded and pruned trees, never follows symlinks (temp dir only)", () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "wph-macos-walk-"));
    const outside = fs.mkdtempSync(path.join(os.tmpdir(), "wph-macos-target-"));
    try {
      // ctime and birthtime cannot be set back, so "old" is a file made before `since`
      fs.writeFileSync(path.join(root, "old.txt"), "x");
      const since = Date.now() + 5;
      while (Date.now() < since + 20) { /* let the clock pass since */ }
      fs.mkdirSync(path.join(root, "Library", "Caches", "ai.citrate.core"), { recursive: true });
      fs.writeFileSync(path.join(root, "Library", "Caches", "ai.citrate.core", "x"), "x");
      fs.mkdirSync(path.join(root, "excluded"));
      fs.writeFileSync(path.join(root, "excluded", "citrate.log"), "x");
      fs.mkdirSync(path.join(root, "Documents"));
      fs.writeFileSync(path.join(root, "Documents", "citrate.txt"), "x");
      fs.writeFileSync(path.join(outside, "citrate-behind-link"), "x");
      fs.symlinkSync(outside, path.join(root, "link"));
      const w = H.walkNewer(root, since, { maxDepth: 5, exclude: [path.join(root, "excluded")], prune: ["Documents"] });
      const rel = w.found.map((p) => path.relative(root, p)).sort();
      expect(rel).not.toContain("old.txt");
      expect(rel).toContain(path.join("Library", "Caches", "ai.citrate.core", "x"));
      expect(rel.some((p) => p.startsWith("excluded"))).toBe(false);
      expect(rel).not.toContain(path.join("Documents", "citrate.txt"));
      expect(rel.some((p) => p.includes("citrate-behind-link"))).toBe(false);
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
      fs.rmSync(outside, { recursive: true, force: true });
    }
  });
});

// ------------------------------------------------------------------ keychain
describe("keychain", () => {
  const DUMP = `keychain: "/r/home/Library/Keychains/login.keychain-db"
version: 512
class: "genp"
attributes:
    0x00000007 <blob>="ai.citrate.core"
    "acct"<blob>="node-storage-key"
    "svce"<blob>="ai.citrate.core"
keychain: "/r/home/Library/Keychains/login.keychain-db"
version: 512
class: "genp"
attributes:
    "acct"<blob>=0x6162  "ab"
    "svce"<blob>=0x61692E63  "ai.citrate.core.custody"
keychain: "/Users/t/Library/Keychains/login.keychain-db"
version: 512
class: "inet"
attributes:
    "acct"<blob>="t"
    "srvr"<blob>="github.com"
`;
  it("parses services and accounts in string and hex form", () => {
    const k = H.parseKeychainDump(DUMP);
    expect(k.map((i) => [i.cls, i.service, i.account])).toEqual([
      ["genp", "ai.citrate.core", "node-storage-key"], ["genp", "ai.citrate.core.custody", "ab"], ["inet", "", "t"],
    ]);
    expect(H.parseKeychainList('    "/r/home/Library/Keychains/login.keychain-db"\n    "/Library/Keychains/System.keychain"\n'))
      .toEqual(["/r/home/Library/Keychains/login.keychain-db", "/Library/Keychains/System.keychain"]);
  });
  it("only items the run added count, and only documented services may remain", () => {
    const all = H.parseKeychainDump(DUMP);
    const d = H.diffKeychain([all[2]], [...all, { keychain: "/k", cls: "genp", service: "com.example.other", account: "x" }]);
    expect(d.added).toHaveLength(3);
    expect(d.removed).toHaveLength(0);
    const doc = [{ service: "ai.citrate.core" }, { service: "ai.citrate.core.custody" }];
    const e = H.evaluateKeychain(d.added, doc);
    expect(e.ok).toBe(false);
    expect(e.undocumented.map((i) => i.service)).toEqual(["com.example.other"]);
    expect(H.evaluateKeychain(d.added.slice(0, 2), doc).ok).toBe(true);
  });
});

// ------------------------------------------------------------------ machine-safety guard
const cleanFacts = () => ({
  platform: "darwin", uid: 501, existingOwnerPaths: [], spotlightApps: [],
  procs: [{ pid: 1, ppid: 0, path: "/sbin/launchd" }], listeners: [{ pid: 9, command: "sshd", proto: "tcp", local: "*:22" }],
  keychain: { "ai.citrate.core": "absent", "ai.citrate.core.custody": "absent" }, signingIdentities: [],
});

describe("machine-safety guard", () => {
  it("lets a clean test machine through", () => {
    expect(H.evaluateGuards(cleanFacts())).toEqual({ ok: true, refusals: [] });
  });
  it.each([
    ["not macOS", { platform: "linux" }, /not macOS/],
    ["root", { uid: 0 }, /root/],
    ["an installed app", { existingOwnerPaths: ["/Applications/Citrate Core.app"] }, /installed or has data here: \/Applications\/Citrate Core.app/],
    ["the owner's data dir", { existingOwnerPaths: ["/Users/o/Library/Application Support/ai.citrate.core"] }, /Application Support\/ai.citrate.core/],
    ["a renamed copy Spotlight knows", { spotlightApps: ["/Applications/Old Citrate.app"] }, /bundle with id ai.citrate.core/],
    ["a running Citrate Core", { procs: [{ pid: 77, ppid: 1, path: "/Applications/Citrate Core.app/Contents/MacOS/citrate" }] }, /running: pid 77/],
    ["the owner's node on 8545", { listeners: [{ pid: 88, command: "citrate", proto: "tcp", local: "127.0.0.1:8545" }] }, /port 8545 is in use/],
    ["a P2P port in use", { listeners: [{ pid: 88, command: "citrate", proto: "tcp", local: "*:30303" }] }, /port 30303/],
    ["owner keychain items", { keychain: { "ai.citrate.core": "present", "ai.citrate.core.custody": "absent" } }, /keychain holds a ai.citrate.core item/],
    ["a release signing identity", { signingIdentities: ["Developer ID Application: Example Inc. (ABCDE12345)"] }, /release machine/],
  ])("refuses %s", (_n, patch, re) => {
    const g = H.evaluateGuards({ ...cleanFacts(), ...patch });
    expect(g.ok).toBe(false);
    expect(g.refusals.join("\n")).toMatch(re);
  });
  it("fails closed when a probe did not run", () => {
    for (const k of ["existingOwnerPaths", "procs", "listeners", "signingIdentities"]) {
      const f = cleanFacts(); delete f[k];
      expect(H.evaluateGuards(f).ok, k).toBe(false);
    }
    expect(H.evaluateGuards({ ...cleanFacts(), keychain: { "ai.citrate.core": "error", "ai.citrate.core.custody": "absent" } }).ok).toBe(false);
    expect(H.evaluateGuards({ ...cleanFacts(), keychain: {} }).ok).toBe(false);
    // Spotlight is advisory: indexing may be off
    expect(H.evaluateGuards({ ...cleanFacts(), spotlightApps: null }).ok).toBe(true);
  });
  it("owner paths cover the installed app and the data under the real home", () => {
    expect(H.ownerPaths("/Users/o")).toEqual(expect.arrayContaining([
      "/Applications/Citrate Core.app", "/Users/o/Applications/Citrate Core.app",
      "/Users/o/Library/Application Support/ai.citrate.core", "/Users/o/.citrate",
      "/Users/o/Library/Logs/ai.citrate.core", "/Users/o/Library/HTTPStorages/ai.citrate.core",
      "/Users/o/Library/HTTPStorages/ai.citrate.core.binarycookies", "/Users/o/Library/Saved Application State/ai.citrate.core.savedState",
    ]));
  });
  it("prints each refusal once, one line per busy node port", () => {
    const listeners = [];
    for (let i = 0; i < 16; i++) listeners.push({ pid: 88, command: "citrate", proto: "tcp", local: `127.0.0.1:8545` });
    listeners.push({ pid: 89, command: "citrate", proto: "tcp", local: "*:30303" });
    const g = H.evaluateGuards({ ...cleanFacts(), listeners, existingOwnerPaths: ["/A", "/A"] });
    expect(g.refusals.filter((r) => /port 8545/.test(r))).toEqual(["port 8545 is in use (pid 88 citrate): a node runs on this machine"]);
    expect(g.refusals.filter((r) => /port 30303/.test(r))).toHaveLength(1);
    expect(g.refusals.filter((r) => /here: \/A$/.test(r))).toHaveLength(1);
    expect(new Set(g.refusals).size).toBe(g.refusals.length);
  });
});

// Fake dependencies: the filesystem, commands and process control are all recorded, none is real.
function fakeDeps({ platform = "darwin", existing = [], ps = "    1     0 /sbin/launchd\n", lsof = "", lsofCode = 1, kc = 44,
  identities = "     0 valid identities found\n", env = { [H.MARKER_ENV]: "1" }, pwHome = "/Users/o", hostname = "acc-mini",
  hosts = hostsFile("acc-mini\n") } = {}) {
  const calls = [];
  const deps = {
    platform, arch: "arm64", uid: 501, pwHome, hostname, env,
    exists: (p) => existing.includes(p),
    readHostsFile: (p) => { calls.push(["READ-HOSTS", p]); return hosts; },
    exec: (cmd, args) => {
      calls.push([cmd, ...args]);
      if (cmd === "ps") return { code: 0, stdout: ps, stderr: "" };
      if (cmd === "lsof") return { code: lsofCode, stdout: lsof, stderr: "" };
      if (cmd === "mdfind") return { code: 0, stdout: "", stderr: "" };
      if (cmd === "security" && args[0] === "find-identity") return { code: 0, stdout: identities, stderr: "" };
      if (cmd === "security") return { code: kc, stdout: "", stderr: "" };
      return { code: 0, stdout: "", stderr: "" };
    },
    spawnApp: () => { calls.push(["SPAWN-APP"]); return 4242; },
    kill: (pid) => { calls.push(["KILL", pid]); },
    out: () => {}, err: () => {},
  };
  return { deps, calls };
}

describe("probes", () => {
  it("look for the owner's data under the password-database home, whatever HOME says", () => {
    const { deps } = fakeDeps({ existing: ["/Users/o/Library/Application Support/ai.citrate.core"], env: { HOME: "/tmp/elsewhere" } });
    const rn = H.createRunner(deps.exec);
    const f = H.gatherGuardFacts({ ...deps, read: rn.read });
    expect(f.existingOwnerPaths).toEqual(["/Users/o/Library/Application Support/ai.citrate.core"]);
    expect(H.evaluateGuards(f).ok).toBe(false);
  });
  it("map the keychain lookup exit codes (0 present, 44 absent, other error) and lsof's empty exit 1", () => {
    for (const [code, want] of [[0, "present"], [44, "absent"], [51, "error"]]) {
      const { deps } = fakeDeps({ kc: code });
      const rn = H.createRunner(deps.exec);
      const f = H.gatherGuardFacts({ ...deps, read: rn.read });
      expect(f.keychain["ai.citrate.core"]).toBe(want);
      expect(f.listeners).toEqual([]);
      expect(f.signingIdentities).toEqual([]);
    }
  });
  it("read lsof's exit 1 (one selection matched nothing) as a valid answer, with or without output; other codes fail closed", () => {
    expect(H.lsofAnswered({ code: 1, stdout: "p88\ncsshd\nPTCP\nn*:22\n", stderr: "" })).toBe(true);
    expect(H.lsofAnswered({ code: 1, stdout: "", stderr: "" })).toBe(true);
    expect(H.lsofAnswered({ code: 0, stdout: "", stderr: "" })).toBe(true);
    expect(H.lsofAnswered({ code: 2, stdout: "", stderr: "lsof: bad option" })).toBe(false);
    expect(H.lsofAnswered({ code: -1, stdout: "", stderr: "ENOENT" })).toBe(false);
    const { deps } = fakeDeps({ lsof: "p88\nccitrate\nf3\nPTCP\nn127.0.0.1:8545\n", lsofCode: 1 });
    const rn = H.createRunner(deps.exec);
    const f = H.gatherGuardFacts({ ...deps, read: rn.read });
    expect(f.listeners).toHaveLength(1);
    expect(H.evaluateGuards(f).refusals.join()).toMatch(/port 8545 is in use/);
  });
  it("see a Developer ID signing identity", () => {
    const { deps } = fakeDeps({ identities: '  1) 89AB "Developer ID Application: Example Inc. (ABCDE12345)"\n' });
    const rn = H.createRunner(deps.exec);
    expect(H.evaluateGuards(H.gatherGuardFacts({ ...deps, read: rn.read })).refusals.join()).toMatch(/release machine/);
  });
});

describe("guarded runner", () => {
  it("refuses every mutation until the guards have passed", () => {
    const seen = [];
    const rn = H.createRunner((c, a) => { seen.push([c, ...a]); return { code: 0, stdout: "", stderr: "" }; });
    for (const [c, a] of [["hdiutil", ["attach", "x.dmg"]], ["ditto", ["a", "b"]], ["rm", ["-rf", "--", "/x"]], ["osascript", ["-e", "quit"]],
      ["xattr", ["-w", "q", "v", "/x"]], ["security", ["default-keychain", "-d", "user", "-s", "/k"]], ["security", ["create-keychain", "-p", "p", "/k"]],
      ["xcrun", ["clang", "-o", "x", "x.c"]], ["codesign", ["--force", "--deep", "-s", "-", "/x"]]]) {
      expect(() => rn.mut(c, a)).toThrow(/before the machine-safety guards/);
    }
    expect(seen).toEqual([]);
    rn.markGuarded();
    rn.mut("ditto", ["a", "b"]);
    expect(seen).toEqual([["ditto", "a", "b"]]);
  });
  it("read() takes only read-only commands", () => {
    const rn = H.createRunner(() => ({ code: 0, stdout: "", stderr: "" }));
    for (const [c, a] of [["ps", ["-ax"]], ["lsof", ["-nP"]], ["codesign", ["--verify", "x"]], ["codesign", ["-dvvv", "x"]], ["spctl", ["--assess", "x"]],
      ["xcrun", ["stapler", "validate", "x"]], ["security", ["find-generic-password", "-s", "s"]], ["security", ["find-identity", "-v", "-p", "codesigning"]],
      ["security", ["list-keychains", "-d", "user"]], ["hdiutil", ["verify", "x"]], ["hdiutil", ["imageinfo", "x"]],
      ["plutil", ["-extract", "k", "raw", "-o", "-", "p"]], ["git", ["rev-parse", "HEAD"]]]) {
      expect(() => rn.read(c, a), `${c} ${a[0]}`).not.toThrow();
    }
    for (const [c, a] of [["hdiutil", ["attach", "x"]], ["hdiutil", ["create", "x"]], ["codesign", ["--force", "-s", "-", "x"]], ["security", ["list-keychains", "-d", "user", "-s", "/k"]],
      ["security", ["delete-generic-password", "-s", "s"]], ["xcrun", ["stapler", "staple", "x"]], ["xcrun", ["clang", "x.c"]], ["spctl", ["--master-disable"]],
      ["git", ["checkout", "x"]], ["rm", ["-rf", "/"]], ["/bin/rm", ["x"]],
      ["security", ["find-generic-password", "-s", "s", "-g"]], ["security", ["find-generic-password", "-w", "-s", "s"]], ["security", ["find-generic-password", "-gs", "s"]],
      ["security", ["dump-keychain", "-d", "/k"]], ["security", ["dump-keychain", "-r", "/k"]],
      ["plutil", ["-extract", "k", "raw", "-o", "/tmp/out", "p"]], ["plutil", ["-extract", "k", "raw", "p"]],
      ["plutil", ["-extract", "k", "raw", "-o", "-", "-o", "/tmp/x", "p"]], ["plutil", ["-extract", "k", "raw", "-o", "-", "-r", "p"]]]) {
      expect(() => rn.read(c, a), `${c} ${a.join(" ")}`).toThrow(/not a read-only command/);
    }
  });
});

describe("main refuses before touching anything", () => {
  const MUTATING = new Set(["ditto", "rm", "osascript", "xattr", "SPAWN-APP", "KILL"]);
  const run = async (opts, argv) => {
    const work = fs.mkdtempSync(path.join(os.tmpdir(), "wph-macos-main-"));
    const { deps, calls } = fakeDeps(opts);
    try {
      const code = await H.main(argv ?? ["--dmg", "/x/Citrate.dmg", "--sha256", SHA, "--mode", "full", "--work", work], deps);
      return { code, calls, workEntries: fs.readdirSync(work) };
    } finally {
      fs.rmSync(work, { recursive: true, force: true });
    }
  };
  it.each([
    ["without the env marker", { env: {} }],
    ["without a hosts file", { hosts: null }],
    ["on a host the hosts file does not list", { hostname: "release-mac" }],
    ["with a world-writable hosts file", { hosts: hostsFile("acc-mini\n", { mode: 0o100666 }) }],
    ["on Linux", { platform: "linux" }],
  ])("exit 2 %s, and no command runs at all", async (_n, opts) => {
    const r = await run(opts);
    expect(r.code).toBe(2);
    expect(r.calls.filter(([c]) => c !== "READ-HOSTS")).toEqual([]);
    expect(r.workEntries).toEqual([]);
  });
  it.each([
    ["on the release Mac (app installed)", { existing: ["/Applications/Citrate Core.app"] }],
    ["with the owner's data dir", { existing: ["/Users/o/Library/Application Support/ai.citrate.core"] }],
    ["with the owner's node on 8545", { lsof: "p88\nccitrate\nf3\nPTCP\nn127.0.0.1:8545\n", lsofCode: 0 }],
    ["with owner keychain items", { kc: 0 }],
    ["with a release signing identity", { identities: '  1) 89AB "Developer ID Application: Example Inc. (ABCDE12345)"\n' }],
  ])("exit 2 %s even with the marker; only read-only probes run, the work dir stays empty", async (_n, opts) => {
    const r = await run(opts);
    expect(r.code).toBe(2);
    expect(r.calls.filter(([c]) => MUTATING.has(path.basename(c)))).toEqual([]);
    for (const [c, ...a] of r.calls.filter(([x]) => x !== "READ-HOSTS")) expect(H.isReadOnly(c, a), `${c} ${a.join(" ")}`).toBe(true);
    expect(r.workEntries).toEqual([]);
  });
  it("exit 2 on a usage error before any probe", async () => {
    const r = await run({}, ["--dmg"]);
    expect(r.code).toBe(2);
    expect(r.calls).toEqual([]);
  });
});

describe("tripwire: the real CLI refuses on a machine without the test-machine marker", () => {
  it("exits 2 with REFUSED and runs nothing (this machine, marker unset)", () => {
    const env = { ...process.env };
    delete env[H.MARKER_ENV];
    const r = spawnSync(process.execPath, [path.join(here, "macos-acceptance.mjs"), "--dmg", "/nonexistent/Citrate.dmg", "--sha256", SHA, "--mode", "full"],
      { env, encoding: "utf8", timeout: 30000 });
    expect(r.status, r.stderr).toBe(2);
    expect(r.stderr).toMatch(/^REFUSED/m);
    expect(r.stderr).toMatch(/CITRATE_MACOS_ACCEPTANCE_TEST_MACHINE=1 is not set/);
    expect(JSON.parse(r.stdout.trim().split("\n").pop())).toMatchObject({ overall: "refused" });
  });
  it("the self-test is refused the same way", () => {
    const env = { ...process.env };
    delete env[H.MARKER_ENV];
    const r = spawnSync(process.execPath, [path.join(here, "macos-acceptance.mjs"), "--self-test"], { env, encoding: "utf8", timeout: 30000 });
    expect(r.status).toBe(2);
    expect(r.stderr).toMatch(/^REFUSED/m);
  });
  it("main checks the marker before it builds a runner or probes anything", () => {
    const src = read(path.join(here, "macos-acceptance.mjs"));
    const body = src.slice(src.indexOf("export async function main("));
    const iMarker = body.indexOf("evaluateMarker(");
    expect(iMarker).toBeGreaterThan(0);
    expect(iMarker).toBeLessThan(body.indexOf("createRunner("));
    expect(iMarker).toBeLessThan(body.indexOf("gatherGuardFacts("));
    // no other way in: no CI-runner bypass, no flag
    expect(src).not.toMatch(/RUNNER_ENVIRONMENT|GITHUB_ACTIONS|testMachineFlag/);
  });
});

// ------------------------------------------------------------------ network profile and RPC
describe("network profile and RPC", () => {
  const G = "0x" + "1d".repeat(32);
  const prof = (p, active = "a") => ({ active, profiles: { a: { chainId: 40204, genesisHash: G, ...p } } });
  it("reads the active profile's chain id and genesis, and refuses one that is not ready", () => {
    expect(H.activeNetwork(prof({}))).toEqual({ id: "a", chainId: 40204, genesisHash: G });
    expect(H.activeNetwork(prof({ genesisHash: G.toUpperCase().replace("0X", "0x") })).genesisHash).toBe(G);
    expect(() => H.activeNetwork(prof({ genesisHash: null }))).toThrow(/not ready/);
    expect(() => H.activeNetwork(prof({ chainId: null }))).toThrow(/not ready/);
    expect(() => H.activeNetwork(prof({ genesisHash: "0x" + "0".repeat(64) }))).toThrow(/not ready/);
    expect(() => H.activeNetwork(prof({}, "missing"))).toThrow(/not in profiles.json/);
  });
  it("the node must answer the profile's chain id AND genesis (a rerolled genesis keeps 40204)", () => {
    const net = { id: "a", chainId: 40204, genesisHash: G };
    expect(H.evaluateRpc({ chainId: 40204, genesisHash: G.toUpperCase().replace("0X", "0x") }, net).ok).toBe(true);
    expect(H.evaluateRpc({ chainId: 40204, genesisHash: "0x" + "2e".repeat(32) }, net).why.join()).toMatch(/genesis 0x2e.* != 0x1d.*profile a/);
    expect(H.evaluateRpc({ chainId: 1, genesisHash: G }, net).why.join()).toMatch(/eth_chainId 1 != 40204/);
    expect(H.evaluateRpc({}, net).why).toEqual(["no eth_chainId answer", "no genesis block (eth_getBlockByNumber 0x0)"]);
  });
  it("this checkout's release spec carries its active profile (the one the app reads)", () => {
    const profiles = JSON.parse(read(path.join(repo, H.NETWORK_PROFILES)));
    const spec = H.loadReleaseSpec(repo);
    expect(spec.network).toEqual(H.activeNetwork(profiles));
    expect(read(path.join(repo, "src-tauri", "src", "network_profile.rs"))).toMatch(/include_str!\("\.\.\/network\/profiles\.json"\)/);
  });
});

// ------------------------------------------------------------------ verdict
describe("verdict", () => {
  const all = (ids, status = "pass") => ids.map((id) => ({ id, status }));
  it("each mode has its required checks, each a superset of the one before", () => {
    expect(H.REQUIRED.inspect).toEqual(["dmg.sha256", "dmg.verify"]);
    expect(H.REQUIRED.verify).toEqual(expect.arrayContaining([...H.REQUIRED.inspect, "dmg.mount", "bundle.structure", "sidecar.digests", "gatekeeper.spctl", "notarization.staple"]));
    expect(H.REQUIRED.full).toEqual(expect.arrayContaining([...H.REQUIRED.verify, "launch.ready", "quit.clean", "reap.orphans", "data.locations", "uninstall.outside_files", "uninstall.keychain"]));
    expect(H.REQUIRED.inspect.some((c) => /mount|install|launch/.test(c))).toBe(false);
  });
  it("a run passes only when every required check of its mode ran and passed", () => {
    for (const mode of H.MODES) {
      expect(H.computeOverall(all(H.REQUIRED[mode]), { mode }), mode).toBe("pass");
      expect(H.computeOverall(all(H.REQUIRED[mode].slice(1)), { mode }), mode).toBe("error");
    }
    expect(H.computeOverall(all(H.REQUIRED.verify), { mode: "full" })).toBe("error");
    const skipped = all(H.REQUIRED.full).map((c) => (c.id === "gatekeeper.spctl" ? { ...c, status: "skip" } : c));
    expect(H.computeOverall(skipped, { mode: "full" })).toBe("error");
    expect(H.computeOverall([...all(H.REQUIRED.full), { id: "dmg.signature", status: "info" }], { mode: "full" })).toBe("pass");
    expect(H.computeOverall(all(H.REQUIRED.full), { mode: "bogus" })).toBe("error");
  });
  it("any fail fails, any error is an error, and a smoke run is never a plain pass", () => {
    expect(H.computeOverall([...all(H.REQUIRED.full), { id: "quit.clean", status: "fail" }], { mode: "full" })).toBe("fail");
    expect(H.computeOverall([{ id: "a", status: "fail" }, { id: "b", status: "error" }], { mode: "full" })).toBe("error");
    const smoke = all(H.REQUIRED.full).map((c) => (c.id.startsWith("sign.") ? { ...c, status: "skip" } : c));
    expect(H.computeOverall(smoke, { mode: "full", unsignedSmoke: true })).toBe("pass-unsigned-smoke");
    expect(H.computeOverall(smoke, { mode: "full" })).toBe("error");
    expect(["pass", "pass-unsigned-smoke", "fail", "error", "refused"].map(H.exitCodeFor)).toEqual([0, 0, 1, 2, 2]);
  });
  it("only a release-mode full pass is G1 evidence", () => {
    expect(H.isG1Evidence({ mode: "full", unsigned_smoke: false, overall: "pass" })).toBe(true);
    expect(H.isG1Evidence({ mode: "verify", unsigned_smoke: false, overall: "pass" })).toBe(false);
    expect(H.isG1Evidence({ mode: "inspect", unsigned_smoke: false, overall: "pass" })).toBe(false);
    expect(H.isG1Evidence({ mode: "full", unsigned_smoke: true, overall: "pass-unsigned-smoke" })).toBe(false);
    expect(H.isG1Evidence({ mode: "full", unsigned_smoke: false, overall: "fail" })).toBe(false);
  });
  it("the self-test judge wants exactly the expected failures", () => {
    const v = (overall, failed) => ({ overall, checks: [{ id: "launch.ready", status: "pass" }, ...failed.map((id) => ({ id, status: "fail" }))] });
    expect(H.judgeSelfTest([], v("pass-unsigned-smoke", [])).ok).toBe(true);
    expect(H.judgeSelfTest(["quit.clean"], v("fail", ["quit.clean"])).ok).toBe(true);
    expect(H.judgeSelfTest(["quit.clean"], v("fail", ["quit.clean", "reap.orphans"])).ok).toBe(false);
    expect(H.judgeSelfTest(["quit.clean"], v("pass-unsigned-smoke", [])).ok).toBe(false);
    expect(H.judgeSelfTest([], v("pass", [])).ok).toBe(false); // a stand-in run is never a release pass
  });
  it("every self-test mode breaks exactly one required check of the mode it runs in", () => {
    const modes = H.SELF_TEST_MODES;
    expect(modes[0]).toEqual({ mode: "good", run: "full", expectFail: [] });
    for (const m of modes.slice(1)) {
      // one check per mode, except a leaked sidecar that still holds a socket: both quit checks
      expect(m.expectFail, m.mode).toEqual(m.mode === "leak" ? ["quit.clean", "quit.listeners"] : [expect.any(String)]);
      for (const c of m.expectFail) expect(H.REQUIRED[m.run], m.mode).toContain(c);
    }
    expect(new Set(modes.map((m) => m.mode)).size).toBe(modes.length);
  });
  it("the stand-in app is valid bash and its sidecar is plain C (no shell, no Citrate code)", () => {
    const r = spawnSync("bash", ["-n"], { input: H.STAND_IN_APP, encoding: "utf8" });
    expect(r.status, r.stderr).toBe(0);
    expect(H.STAND_IN_APP).toMatch(/^#!\/bin\/bash\n/);
    for (const m of H.SELF_TEST_MODES.filter((x) => x.run === "full" && x.mode !== "good")) {
      expect(H.STAND_IN_APP, m.mode).toMatch(new RegExp(`"\\$MODE" !?= ${m.mode}\\b`));
    }
    // leak mode leaks on the first quit only, so it breaks quit.clean and not the later reap.quit
    expect(H.STAND_IN_APP).toMatch(/"\$MODE" = leak \] && \[ ! -f "\$HOME\/\.selftest-leaked" \]/);
    expect(H.STAND_IN_SIDECAR_C).toMatch(/int main\(int argc, char \*\*argv\)/);
    expect(H.STAND_IN_SIDECAR_C).not.toMatch(/system\(|exec[lv]p?\(/);
  });
});

// ------------------------------------------------------------------ tripwires
describe("tripwires", () => {
  const rustFiles = (dir) => {
    const out = [];
    const walk = (d) => { for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p); else if (e.name.endsWith(".rs") && !/_tests?\.rs$/.test(e.name)) out.push(p);
    } };
    walk(dir);
    return out;
  };
  it("every keychain service the app names is documented, and every documented one is used", () => {
    const used = new Set();
    for (const f of [...rustFiles(path.join(repo, "src-tauri", "src")), ...rustFiles(path.join(repo, "kit", "src"))]) {
      const s = fs.readFileSync(f, "utf8");
      for (const m of s.matchAll(/(?:KEYRING_SERVICE|CUSTODY_KEYRING_SERVICE|LEGACY_SERVICE|CUSTODY_SERVICE)\s*:\s*&(?:'static\s+)?str\s*=\s*"([^"]+)"/g)) used.add(m[1]);
      for (const m of s.matchAll(/keyring::Entry::new\(\s*"([^"]+)"/g)) used.add(m[1]);
    }
    const doc = H.parseAllowlist(read(path.join(here, "macos-acceptance", "keychain-allowlist.txt")), ["service", "reason"]).map((d) => d.service);
    expect([...used].sort()).toEqual(["ai.citrate.core", "ai.citrate.core.custody"]);
    expect(doc.sort()).toEqual([...used].sort());
    expect(H.KEYCHAIN_SERVICES.slice().sort()).toEqual([...used].sort());
  });
  it("the app's data roots (local_data.rs app_roots) are the documented locations", () => {
    const src = read(path.join(repo, "src-tauri", "src", "local_data.rs"));
    const fn = src.slice(src.indexOf("fn app_roots"), src.indexOf("fn app_roots") + 1400);
    const dirs = [...fn.matchAll(/p\.(app_[a-z_]+_dir)\(\)/g)].map((m) => m[1]).sort();
    expect(dirs).toEqual(["app_cache_dir", "app_config_dir", "app_data_dir", "app_local_data_dir", "app_log_dir"]);
    expect(fn).toMatch(/join\("Library"\)\.join\("WebKit"\)\.join\(BUNDLE_ID\)/);
    const doc = H.parseLocations(read(path.join(here, "macos-acceptance", "data-locations.txt"))).map((d) => d.prefix);
    for (const p of ["Library/Application Support/ai.citrate.core", "Library/Caches/ai.citrate.core", "Library/Logs/ai.citrate.core", "Library/WebKit/ai.citrate.core"]) {
      expect(doc).toContain(p);
    }
  });
  it("the harness's bundle id, product and spec follow the app and the release workflow", () => {
    const conf = JSON.parse(read(path.join(repo, "src-tauri", "tauri.conf.json")));
    expect(conf.identifier).toBe(H.BUNDLE_ID);
    expect(`${conf.productName}.app`).toBe(H.APP_DIR_NAME);
    const rel = read(path.join(repo, ".github", "workflows", "release.yml"));
    const overlays = [...rel.matchAll(/--config (src-tauri\/tauri\.[a-z-]+\.conf\.json)/g)].map((m) => m[1]);
    expect(overlays.length).toBeGreaterThan(0);
    for (const o of overlays) expect(o).toBe(H.RELEASE_OVERLAY);
    const overlay = JSON.parse(read(path.join(repo, H.RELEASE_OVERLAY)));
    const manifest = JSON.parse(read(path.join(repo, "release", "components.json")));
    expect(manifest.components.map((c) => c.external_bin).sort()).toEqual([...overlay.bundle.externalBin].sort());
  });
  it("every sidecar the listener policy names is one the release bundles", () => {
    const spec = H.loadReleaseSpec(repo);
    const policy = H.parseListenerPolicy(read(path.join(here, "macos-acceptance", "listener-policy.txt")));
    for (const r of policy) expect(spec.bins, r.exe).toContain(r.exe);
    expect(fs.existsSync(path.join(here, "macos-acceptance", "wildcard-allowlist.txt"))).toBe(false);
  });
  it("process control and command execution live only in realDeps", () => {
    const src = read(path.join(here, "macos-acceptance.mjs"));
    const start = src.indexOf("export function realDeps()");
    expect(start).toBeGreaterThan(0);
    const before = src.slice(0, start).replace(/^import .*$/gm, "");
    expect(before).not.toMatch(/\bspawnSync\(|\bspawn\(|\bexecSync\(|\bexecFileSync\(|process\.kill\(/);
  });
  it("the workflow runs the harness only on macOS and never puts a dispatch input in a script", () => {
    const wf = read(path.join(repo, ".github", "workflows", "macos-acceptance.yml"));
    expect(wf).toMatch(/workflow_dispatch:/);
    const runsOn = [...wf.matchAll(/runs-on:\s*(\S+)/g)].map((m) => m[1]);
    expect(runsOn.length).toBeGreaterThan(0);
    for (const r of runsOn) expect(r).toMatch(/^macos-/);
    const runBlocks = [...wf.matchAll(/^\s+run: \|\n((?:\s{10,}.*\n?)+)/gm)].map((m) => m[1]).join("\n");
    expect(runBlocks.length).toBeGreaterThan(0);
    expect(runBlocks).not.toMatch(/\$\{\{\s*(inputs|github\.event)\./);
    expect(wf).toMatch(/persist-credentials: false/);
    expect(wf).toMatch(/permissions:\s*\n\s+contents: read/);
  });
});
