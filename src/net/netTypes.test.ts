// L7-WP01 (lanes/L7_NETWORKING.md section 4, L7-WP01 RED test): the TS link vocabulary in
// src/bridge/lanes/l7.ts names exactly what core serializes. Core checks the same fixture against
// its enums (src-tauri/src/net/link_security_tests.rs `fixture_json`), so a rename on either side
// fails one of the two. Written red-first: the fixture and the types were absent.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { LinkKind, LinkSecurity, NetStatus } from "../bridge/lanes/l7";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const fixture = JSON.parse(readFileSync(join(ROOT, "src-tauri/src/net/fixtures/net_status_v1.json"), "utf8"));

// Written out against the TS unions: adding a union member without a row here, or the reverse,
// is caught by the comparison with the fixture below.
const LINK_KINDS: Record<LinkKind, true> = {
  "tailnet-own": true,
  "tailnet-managed": true,
  warp: true,
  lan: true,
  none: true,
};
const LINK_SECURITY: Record<LinkSecurity, true> = {
  "pq-hybrid-e2e": true,
  "classical-e2e": true,
  "terminated-at-provider": true,
  none: true,
};
const NET_STATUS_KEYS: Record<keyof NetStatus, true> = {
  link: true,
  tailscale: true,
  tailscaleVia: true,
  warp: true,
  privateAddrs: true,
  pairingPort: true,
  mesh: true,
  meshLinkSecurity: true,
  note: true,
};

const sorted = (xs: string[]) => [...xs].sort();

describe("L7-WP01 link vocabulary", () => {
  it("link kinds match core's names", () => {
    expect(fixture.version).toBe(1);
    expect(sorted(fixture.linkKinds)).toEqual(sorted(Object.keys(LINK_KINDS)));
  });

  it("link security values match core's names", () => {
    expect(sorted(fixture.linkSecurity)).toEqual(sorted(Object.keys(LINK_SECURITY)));
    // Only one value may ever be shown as post-quantum.
    expect(Object.keys(LINK_SECURITY).filter((v) => v.startsWith("pq-"))).toEqual(["pq-hybrid-e2e"]);
  });

  it("the NetStatus shape matches the fixture", () => {
    const ns = fixture.netStatus;
    expect(sorted(Object.keys(ns))).toEqual(sorted(Object.keys(NET_STATUS_KEYS)));
    expect(Object.keys(LINK_KINDS)).toContain(ns.link);
    expect(Object.keys(LINK_SECURITY)).toContain(ns.meshLinkSecurity);
    expect(["localapi", "cli", "none"]).toContain(ns.tailscaleVia);
    expect(sorted(Object.keys(ns.tailscale))).toEqual(["peers", "selfHost", "selfIps", "state", "version"]);
    expect(sorted(Object.keys(ns.mesh))).toEqual(["note", "on", "signedOff", "source"]);
    expect(sorted(Object.keys(ns.warp))).toEqual(["ips", "present"]);
  });

  it("the suite id matches the one core pins", () => {
    expect(fixture.pqSuite).toBe("X25519MLKEM768-HKDF-SHA384-AES256GCM");
  });
});
