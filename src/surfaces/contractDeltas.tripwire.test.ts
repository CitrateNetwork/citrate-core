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
// 3. The wallet is a key on this device (core#425). No copy anywhere in src/ calls it a smart
//    wallet or account, ERC-4337, a passkey validator or signer, guardian recovery, a UserOp, or
//    something that deploys lazily: 0.5.2 has none of these, and device loss is wallet loss (O-21).
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

// A claim that the member's wallet is something 0.5.2 does not have.
const WALLET_KIND_CLAIM =
  /smart[ -]?(wallet|account)|ERC-?4337|passkey (validator|signer)|guardian (module|recovery)|configure guardians|\bUserOps?\b|deploys? lazily|counterfactual/i;

// Files allowed to use those words, and why. Each entry must say why it is not a claim about the
// member's wallet; a gap in the member's own wallet copy is never listed here.
const WALLET_KIND_KNOWN: Record<string, string> = {
  "data/seed.ts": "chain-facts catalog: describes the deployed EntryPoint contract and the SDK's features, not the member's wallet",
};

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

});

describe("tripwire: the wallet is a key on this device (core#425)", () => {
  it("no copy claims a smart wallet, passkey signer, guardian recovery, UserOp or lazy deployment", () => {
    const offenders: string[] = [];
    for (const f of files(SRC)) {
      const r = rel(f);
      if (WALLET_KIND_KNOWN[r]) continue;
      readFileSync(f, "utf8")
        .split("\n")
        .forEach((line, i) => {
          if (isComment(line)) return;
          if (WALLET_KIND_CLAIM.test(line)) offenders.push(`${r}:${i + 1}: ${line.trim()}`);
        });
    }
    expect(offenders).toEqual([]);
  });

  it("the scanner flags each claim form and skips comments", () => {
    for (const claim of [
      "Smart wallet · ERC-4337",
      "Your smart wallet already exists",
      "Passkey validator (WebAuthn P-256)",
      "Guardian module — configure guardians any time",
      "broadcasting UserOp",
      "Deploys lazily on first outgoing action",
      "the counterfactual address",
    ]) {
      expect(WALLET_KIND_CLAIM.test(claim), claim).toBe(true);
    }
    expect(WALLET_KIND_CLAIM.test("passkey, email, Google, or sign-in-with-Ethereum")).toBe(false);
    expect(isComment("  // the counterfactual smart-wallet address")).toBe(true);
  });

  it("every allowed file still exists and still uses the words (no stale entries)", () => {
    for (const r of Object.keys(WALLET_KIND_KNOWN)) {
      expect(readFileSync(join(SRC, r), "utf8")).toMatch(WALLET_KIND_CLAIM);
    }
  });
});
