// @vitest-environment node
//
// Tripwire, owner decision D-9 (2026-10-26): the liquid staking pool is retired at the r1006 reroll
// and is not in the genesis contract set. Citrate Core makes no pool calls and offers no pool
// self-stake. No pool address, selector, event, command or self-stake field may come back in the
// webview (src/), the core crate (src-tauri/src), its address book or the kit (kit/src).
//
// The banned strings live in fixtures/retired/liquid-staking-pool.json, which the Rust tripwire
// (src-tauri/src/lsp_retired_tests.rs) reads too, so the two lists cannot drift.
import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";

const REPO = join(__dirname, "../..");

interface Retired {
  scan: string[];
  addresses: string[];
  selectors: string[];
  strings: string[];
  allow: Record<string, string[]>;
  allowReasons: Record<string, string>;
  uiCopy: string[];
}

export const RETIRED: Retired = JSON.parse(readFileSync(join(REPO, "fixtures/retired/liquid-staking-pool.json"), "utf8"));

const TEXT = /\.(ts|tsx|js|mjs|cjs|rs|json|toml|css|html|md|txt|sh)$/;

function files(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...files(p));
    else if (TEXT.test(name)) out.push(p);
  }
  return out;
}

/** `a1b2c3d4` as Rust writes a pinned selector: `0xa1, 0xb2, 0xc3, 0xd4`. */
const byteForm = (hex: string) => (hex.match(/../g) ?? []).map((b) => "0x" + b).join(", ");

/** Every banned string in `text`, except those `allowed` for this file. */
export function retiredHits(text: string, allowed: readonly string[] = []): string[] {
  const hits: string[] = [];
  const lower = text.toLowerCase();
  for (const a of RETIRED.addresses) if (lower.includes(a.toLowerCase())) hits.push(a);
  for (const s of RETIRED.selectors) {
    if (lower.includes(s) || lower.includes(byteForm(s))) hits.push(s);
  }
  for (const s of RETIRED.strings) if (!allowed.includes(s) && text.includes(s)) hits.push(s);
  return hits;
}

const self = relative(REPO, __filename).split(sep).join("/");

describe("D-9: the liquid staking pool is gone from core", () => {
  it("the scanner finds each kind of reference (control)", () => {
    expect(retiredHits("to: " + RETIRED.addresses[0].toUpperCase().replace("0X", "0x"))).toHaveLength(1);
    expect(retiredHits("const S: [u8; 4] = [" + byteForm(RETIRED.selectors[0]) + "];")).toEqual([RETIRED.selectors[0]]);
    expect(retiredHits("data: 0x" + RETIRED.selectors[1] + "00")).toEqual([RETIRED.selectors[1]]);
    for (const s of RETIRED.strings) expect(retiredHits("x " + s + " y")).toContain(s);
    expect(retiredHits("x " + RETIRED.strings[0] + " y", [RETIRED.strings[0]])).toEqual([]);
  });

  it("every allowance names a file that exists and gives a reason", () => {
    for (const [f, allowed] of Object.entries(RETIRED.allow)) {
      expect(statSync(join(REPO, f)).isFile()).toBe(true);
      expect(allowed.length).toBeGreaterThan(0);
      expect(RETIRED.allowReasons[f] ?? "").not.toBe("");
    }
  });

  it("no pool address, selector, event, command or self-stake field in src, src-tauri or kit", () => {
    const found: string[] = [];
    let scanned = 0;
    for (const root of RETIRED.scan) {
      for (const p of files(join(REPO, root))) {
        const rel = relative(REPO, p).split(sep).join("/");
        if (rel === self) continue;
        scanned++;
        for (const h of retiredHits(readFileSync(p, "utf8"), RETIRED.allow[rel] ?? [])) found.push(`${rel}: ${h}`);
      }
    }
    expect(scanned).toBeGreaterThan(500);
    expect(found).toEqual([]);
  });
});
