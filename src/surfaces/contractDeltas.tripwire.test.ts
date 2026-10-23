// Tripwires for the UI truth contract and CLAUDE.md Rule 3 (design → core handoff, Wallet and
// Settings deltas).
//
// 1. Money tiles carry no hard-coded balance. A tile value is a read (named source) or the
//    unavailable state "—". A literal like "0.00" in a money surface is a number with no source
//    (contract rules 1 and 2: unknown is not zero). Input placeholders are not values and are
//    skipped.
// 2. No setting can make an intent skip the signature ceremony. The only exception is the budgeted
//    web sign-in (SIWE) in Budgets. So no frontend file reads or writes a signature-policy value,
//    and no copy offers an allowlist that skips the ceremony.
// 3. The wallet is a key on this device; no copy calls it a passkey validator.
import { describe, it, expect } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const SRC = join(__dirname, "..");

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    if (statSync(p).isDirectory()) return files(p);
    return /\.(ts|tsx)$/.test(n) && !/\.test\.(ts|tsx)$/.test(n) ? [p] : [];
  });
}

const rel = (f: string) => relative(SRC, f).split("\\").join("/");
const isComment = (line: string) => /^\s*(\/\/|\/?\*|\{\/\*)/.test(line);

// The surfaces that draw balances, stake, rewards or bonds.
const MONEY_SURFACES = ["surfaces/Wallet.tsx", "surfaces/Dashboard.tsx", "surfaces/Node.tsx"];

// A decimal amount as a string literal or as JSX text, e.g. "0.00", '12.50', >0.00<.
const DECIMAL_LITERAL = /(["'`])\d[\d,]*\.\d+\1|>\s*\d[\d,]*\.\d+\s*</;
// A tile value bound straight to a numeric literal, e.g. const wsaltStr = "0".
const LITERAL_TILE_VALUE = /\bconst\s+\w*(Str|Val|Value|Balance|Amount)\s*=\s*(["'`])\d[\d,.]*\2/;

describe("tripwire: money tiles carry no hard-coded balance", () => {
  it("no decimal balance literal or literal tile value in a money surface", () => {
    const offenders: string[] = [];
    for (const f of MONEY_SURFACES) {
      const src = readFileSync(join(SRC, f), "utf8");
      src.split("\n").forEach((line, i) => {
        if (isComment(line) || /placeholder=/.test(line)) return;
        if (DECIMAL_LITERAL.test(line) || LITERAL_TILE_VALUE.test(line)) offenders.push(`${f}:${i + 1}: ${line.trim()}`);
      });
    }
    expect(offenders).toEqual([]);
  });

  it("the scan bites on the shapes it guards (negative control)", () => {
    expect(DECIMAL_LITERAL.test('  const wsaltStr = "0.00";')).toBe(true);
    expect(DECIMAL_LITERAL.test("<span>0.00</span>")).toBe(true);
    expect(LITERAL_TILE_VALUE.test('const rewardsStr = "0";')).toBe(true);
    expect(DECIMAL_LITERAL.test('const wsaltStr = "—";')).toBe(false);
  });

  it("every money surface listed still exists (no stale entries)", () => {
    for (const f of MONEY_SURFACES) expect(() => statSync(join(SRC, f))).not.toThrow();
  });
});

// Files allowed to mention the forbidden phrases, and why. Each entry is a known gap, not a pass.
// The onboarding S4 card was rewritten in #349 (core#399 wording), so there are no known gaps.
const PASSKEY_KNOWN: Record<string, string> = {};

describe("tripwire: no setting skips the signature ceremony", () => {
  it("no frontend file reads or writes a signature-policy value", () => {
    const offenders: string[] = [];
    for (const f of files(SRC)) {
      readFileSync(f, "utf8")
        .split("\n")
        .forEach((line, i) => {
          if (!/\bsigPolicy\b|\bsig_policy\b/.test(line)) return;
          // The one allowed mention: loadState deleting a value an older build persisted.
          if (rel(f) === "shell/state.ts" && /^\s*if \(saved\) delete \(base as \{ sigPolicy\?: unknown \}\)\.sigPolicy;\s*$/.test(line)) return;
          offenders.push(`${rel(f)}:${i + 1}: ${line.trim()}`);
        });
    }
    expect(offenders).toEqual([]);
  });

  it("no copy offers an allowlist that skips the ceremony", () => {
    const offenders: string[] = [];
    for (const f of files(SRC)) {
      readFileSync(f, "utf8")
        .split("\n")
        .forEach((line, i) => {
          if (isComment(line)) return;
          if (/skip(s)? the (signature )?ceremony|allowlisted intents|allowlist rules/i.test(line)) offenders.push(`${rel(f)}:${i + 1}: ${line.trim()}`);
        });
    }
    expect(offenders).toEqual([]);
  });

  it("no copy calls the wallet a passkey validator", () => {
    const offenders: string[] = [];
    for (const f of files(SRC)) {
      const r = rel(f);
      if (PASSKEY_KNOWN[r]) continue;
      readFileSync(f, "utf8")
        .split("\n")
        .forEach((line, i) => {
          if (isComment(line)) return;
          if (/passkey validator/i.test(line)) offenders.push(`${r}:${i + 1}: ${line.trim()}`);
        });
    }
    expect(offenders).toEqual([]);
  });

  it("every known-gap entry still exists and still has the phrase (no stale entries)", () => {
    for (const r of Object.keys(PASSKEY_KNOWN)) {
      expect(readFileSync(join(SRC, r), "utf8")).toMatch(/passkey validator/i);
    }
  });
});
