// @vitest-environment node
//
// Tripwire (C1-V V5, lead audit of S5 on r1006, 2026-10-27): the membership grant lands in a locked
// MemberBond and is staked only after the member activates it. None of the old custody claims may
// come back anywhere the member or Hermes can read them: the webview (src/), the core crate
// (src-tauri/src, which also writes the seeded memory facts) or the kit. Tests are not scanned;
// they name these phrases to assert their absence.
import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";

const REPO = join(__dirname, "../..");
const ROOTS = ["src", "src-tauri/src", "kit/src"];

/** Main's S5 claims plus the seeded-memory wording, each a phrase that is false on r1006. */
export const GRANT_CLAIMS = [
  "staked for you",
  "arrives already staked",
  "arrives staked",
  "the vault stakes it",
  "SALT staked · settled",
  "Staked position",
  "vaulted principal",
  "until mainnet release",
  "vaulted until mainnet",
  "earns are yours",
  "accrue to you",
  "Grant + stake",
  "grant + stake",
  "stake ceremony",
  "membership grant staked",
  "staked SALT is locked",
];

const isTest = (p: string) => /\.test\.|_tests\.rs$|tripwire/.test(p);

function files(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...files(p));
    else if (/\.(ts|tsx|rs|json|md)$/.test(name) && !isTest(name)) out.push(p);
  }
  return out;
}

export function grantClaimHits(text: string): string[] {
  return GRANT_CLAIMS.filter((c) => text.includes(c));
}

describe("C1-V V5: no false grant-custody claim in src, src-tauri/src or kit/src", () => {
  it("the scanner finds each phrase (control)", () => {
    for (const c of GRANT_CLAIMS) expect(grantClaimHits("x " + c + " y")).toEqual([c]);
    expect(grantClaimHits("Your grant is locked in your bond.")).toEqual([]);
  });

  it("no source file carries one", () => {
    const found: string[] = [];
    let scanned = 0;
    for (const root of ROOTS) {
      for (const p of files(join(REPO, root))) {
        scanned++;
        for (const h of grantClaimHits(readFileSync(p, "utf8"))) found.push(`${relative(REPO, p).split(sep).join("/")}: ${h}`);
      }
    }
    expect(scanned).toBeGreaterThan(400);
    expect(found).toEqual([]);
  });
});
