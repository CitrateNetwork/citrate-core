// CX-S2.4 — StorageFiles copy honesty (D-22 subsidy framing / Rule 1 / compliance).
//
// The storage surface must never frame storage as the USER earning by storing (D-22), never
// guaranteed, never framed as immediate cash, and it must be honest that the on-chain bond is NOT
// live (S2.2 blocked on the chain CommD fix; IPFSIncentivesV3 dormant, UI truth contract §1). These are
// the red-then-green tripwires: they fail if any forbidden earnings claim renders, and prove the
// honest "forthcoming / pin locally" framing holds. Mirrors the copy-lint patterns in
// scripts/cx-copy-lint.sh so the surface and the CI gate agree.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { StorageFiles } from "./StorageFiles";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";

// StorageFiles renders from its slice + `store.toast`; effects don't fire in static markup, so a
// minimal stub is enough.
const stubStore = { toast: () => {} } as unknown as Store;

const html = renderToStaticMarkup(<StorageFiles store={stubStore} s={freshState("p1")} />);

describe("StorageFiles honesty — subsidy-framed, no user-earnings/guaranteed/day-one (D-22)", () => {
  it("never implies the user earns by storing, guarantees, or immediate cash", () => {
    expect(html).not.toMatch(/earn\s+salt\s+by\s+storing/i);
    expect(html).not.toMatch(/guaranteed\s+(salt|rewards?|returns?|income)/i);
    expect(html).not.toMatch(/day.?one\s+(cash|earnings?)/i);
  });

  // K4 (UI truth contract §1): IPFSIncentivesV3 is deployed but dormant and unfunded, so the earlier
  // "staked SALT bond you approve" framing described something that is not live. Pins are local.
  it("says pin bonds are not live yet, in the contract's required words", () => {
    expect(html).toContain("Pin bonds not live yet");
  });

  it("frames pinning as a local pin with no bond and no transaction, never a bond to approve", () => {
    const lower = html.toLowerCase();
    expect(lower).toContain("a local pin, with no bond and no transaction");
    expect(lower).not.toContain("staked salt bond");
    expect(html).not.toContain("Pin · bond");
    expect(lower).not.toMatch(/approve the bond/);
  });
});

// Codex P1 on #368 at a82035c: the local IPFS node is a normal networked kubo node, and added
// files are plaintext and announced by CID. Nothing gates retrieval behind a Share action, so the
// surface must not promise privacy; it says plainly that a CID can be fetched from this node.
const PRIVACY_PROMISE = /nothing (is sent|leaves)|until you (pin or )?share|private until|stays? (private|on this (machine|node) until)/i;

describe("StorageFiles never promises a file stays private (IPFS content is retrievable by CID)", () => {
  it("says that anyone with a file's CID can fetch it from this node", () => {
    expect(html).toContain("anyone with its CID can fetch it from this node while the store is running");
    expect(html).toContain("don't add files you need to keep private");
  });

  it("makes no privacy promise", () => {
    expect(html).not.toMatch(PRIVACY_PROMISE);
  });

  it("tripwire: no storage surface or the storage doc promises privacy for IPFS content", async () => {
    const { readFileSync, readdirSync } = await import("node:fs");
    const { join } = await import("node:path");
    const files = readdirSync(__dirname)
      .filter((f) => /^Storage.*\.tsx$/.test(f) || f === "storageTruth.ts" || f === "storageParts.ts")
      .map((f) => join(__dirname, f));
    files.push(join(__dirname, "../../docs/STORAGE_K4.md"));
    for (const f of files) {
      expect(readFileSync(f, "utf8"), f).not.toMatch(PRIVACY_PROMISE);
    }
  });
});
