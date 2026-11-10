// STD tripwire (port of #366 C1-V5 / chain#331 onto main's bond model). Two classes can recur:
//
//  1. A registry read for the wrong key. The validator words and the producer check must use the
//     member's validator key (MemberBond.pubkey() once the bond has activated, else this node's
//     key), so the store has ONE registry-read site and it takes its key from memberValidatorKey().
//  2. Activation offered outside the bond lock. Every offer of activation goes through the one gate
//     that reads the lock: canOfferActivation(stake, activation, bondActivated, bondUnlocked) for the
//     words-and-offer surfaces, or the S7 step's activationGate / activationPhase (its own read).
//     A caller that drops the lock argument, or a new activation entry that skips both gates, fails.
import { describe, it, expect } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

function sources(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...sources(p));
    else if (/\.(ts|tsx)$/.test(name) && !/\.test\.(ts|tsx)$/.test(name)) out.push(p);
  }
  return out;
}

const SRC = sources("src").map((p) => ({ p, s: readFileSync(p, "utf8") }));

/** The argument lists of every call to `fn(` in `s` (balanced parentheses, top-level commas). */
function callArgs(s: string, fn: string): string[][] {
  const out: string[][] = [];
  const re = new RegExp(`(?<![\\w.])${fn}\\(`, "g");
  let m: RegExpExecArray | null;
  while ((m = re.exec(s))) {
    // Skip the definition itself.
    if (/function\s+$/.test(s.slice(Math.max(0, m.index - 12), m.index))) continue;
    let depth = 1;
    let i = m.index + m[0].length;
    let cur = "";
    const args: string[] = [];
    for (; i < s.length && depth > 0; i++) {
      const c = s[i];
      if (c === "(" || c === "[" || c === "{") depth++;
      if (c === ")" || c === "]" || c === "}") depth--;
      if (depth === 0) break;
      if (c === "," && depth === 1) {
        args.push(cur.trim());
        cur = "";
      } else cur += c;
    }
    if (cur.trim()) args.push(cur.trim());
    out.push(args);
  }
  return out;
}

describe("tripwire: the member's validator key and the bond lock", () => {
  it("the store reads the registry at one site, keyed on memberValidatorKey()", () => {
    const sites = SRC.flatMap(({ p, s }) => (s.match(/chainReads\.validatorActivation\(/g) ?? []).map(() => p));
    expect(sites).toEqual([join("src", "shell", "store.ts")]);
    const store = readFileSync(join("src", "shell", "store.ts"), "utf8");
    const body = store.slice(store.indexOf("private async readValidatorActivation("), store.indexOf("private async memberValidatorKey("));
    expect(body).toContain("await this.memberValidatorKey()");
    expect(body).toMatch(/chainReads\.validatorActivation\(key\b/);
    // The node's proposer key is consulted only inside memberValidatorKey, after the bond's key.
    const key = store.slice(store.indexOf("private async memberValidatorKey("));
    const keyBody = key.slice(0, key.indexOf("\n  }\n"));
    expect(keyBody.indexOf("memberKeyRule(this.state)")).toBeGreaterThan(-1);
    expect(keyBody.indexOf("memberKeyRule(this.state)")).toBeLessThan(keyBody.indexOf("validatorEarnings()"));
    // The rule itself puts MemberBond.pubkey() first.
    const rule = store.slice(store.indexOf("export function memberKeyRule("));
    expect(rule.slice(0, rule.indexOf("\n}\n")).indexOf("s.bondPubkey")).toBeGreaterThan(-1);
    expect(body).not.toContain("validatorEarnings()");
  });

  it("every canOfferActivation call passes the bond lock as read", () => {
    const calls = SRC.flatMap(({ p, s }) => callArgs(s, "canOfferActivation").map((a) => ({ p, a })));
    expect(calls.length).toBeGreaterThanOrEqual(3); // S6, Node, maybeAutoBond
    const short = calls.filter(({ a }) => a.length < 4 || !/[Uu]nlocked/.test(a[3]));
    expect(short.map(({ p, a }) => `${p}: canOfferActivation(${a.join(", ")})`)).toEqual([]);
  });

  it("every validatorWord / validatorDash call outside chainTruth passes the bond reads", () => {
    const bad: string[] = [];
    for (const { p, s } of SRC) {
      if (p.endsWith(join("shell", "chainTruth.ts"))) continue;
      for (const fn of ["validatorWord", "validatorDash"]) {
        for (const a of callArgs(s, fn)) {
          // A registered stake never reaches the bond words; any other call needs both bond reads.
          const stakeOnly = /^s\.bondedStake$/.test(a[0] ?? "") && a.length === 3 && p.endsWith("Onboarding.tsx");
          if (stakeOnly) continue;
          if (a.length < 5 || !/[Aa]ctivated/.test(a[3]) || !/[Uu]nlocked/.test(a[4])) bad.push(`${p}: ${fn}(${a.join(", ")})`);
        }
      }
    }
    expect(bad).toEqual([]);
  });

  it("every activation entry goes through a gate that reads the lock", () => {
    const store = readFileSync(join("src", "shell", "store.ts"), "utf8");
    // Since #378 the one entry is requestValidatorActivation; activateValidator is private and is
    // called from nowhere else, so the entry's gate (which has the "lock" check) runs first.
    expect(store).toMatch(/private async activateValidator\(/);
    expect(store.match(/this\.activateValidator\(/g)?.length).toBe(1);
    const act = store.slice(store.indexOf("  async requestValidatorActivation("));
    expect(act.slice(0, act.indexOf("this.activateValidator("))).toContain("activationGate(this.state.activation");
    const auto = store.slice(store.indexOf("  private maybeAutoBond("));
    expect(auto.slice(0, auto.indexOf("armMining()"))).toMatch(/canOfferActivation\([^)]*bondUnlocked\)/);
    const gate = readFileSync(join("src", "onboarding", "activateValidator.ts"), "utf8");
    expect(gate).toMatch(/key: "lock"/);
    // Every UI file that can start activation also names one of the gates. K3: the Node page's
    // activationOffer counts only because it decides with canOfferActivation (checked here).
    for (const { p, s } of SRC) {
      if (!/store\.(activateValidator|requestValidatorActivation)\(/.test(s)) continue;
      expect(/canOfferActivation\(|activationGate\(|activationPhase\(|activationOffer\(/.test(s), p).toBe(true);
    }
    const words = readFileSync(join("src", "surfaces", "node", "nodeWords.ts"), "utf8");
    const offer = words.slice(words.indexOf("export function activationOffer("));
    expect(offer.slice(0, offer.indexOf("return { offer: true")), "activationOffer").toMatch(/canOfferActivation\(/);
  });
});
