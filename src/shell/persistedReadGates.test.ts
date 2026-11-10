// STD tripwire (Codex P1 on #366; ported onto main's bond model): a persisted field that gates a
// chain read can come back from localStorage while the observed state it depends on (bondActivated,
// bondPubkey, bondUnlocked) starts empty, so the read never runs after a restart. Every `const needX = …` read gate in the shell is scanned; any PERSIST_KEYS field it
// reads must have a restart test (a "null after restore" case) named here, or a documented reason.
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { PERSIST_KEYS } from "./state";

/** Persisted field → the restart test that covers its read gate (or why none is needed). */
const RESTART_COVERAGE: Record<string, { test: string } | { reason: string }> = {
  // needBond: a restored stake below the bonded threshold re-reads while the node runs; a restored
  // full stake no longer needs the bond read, and the bond reads come through needActivation.
  bondedStake: { test: "with the node stopped and the grant restored from disk, the poll reads an unread activated() flag" },
  hasGrant: { test: "with the node stopped and the grant restored from disk, the poll reads an unread activated() flag" },
};

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return sources(p);
    return /\.(ts|tsx)$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [p] : [];
  });
}

function gatedPersistedFields(src: string): Set<string> {
  const found = new Set<string>();
  // Local aliases of state fields: `let bonded = this.state.bondedStake;`
  const alias = new Map<string, string>();
  for (const m of src.matchAll(/(?:const|let)\s+(\w+)\s*=\s*this\.state\.(\w+)\s*;/g)) alias.set(m[1], m[2]);
  for (const m of src.matchAll(/const\s+need\w*\s*=\s*([^;]+);/g)) {
    const expr = m[1];
    for (const f of expr.matchAll(/this\.state\.(\w+)/g)) found.add(f[1]);
    for (const [a, f] of alias) if (new RegExp(`\\b${a}\\b`).test(expr)) found.add(f);
  }
  return new Set([...found].filter((f) => (PERSIST_KEYS as string[]).includes(f)));
}

describe("STD tripwire — persisted fields that gate a read have a restart test", () => {
  const files = sources("src/shell");
  const gated = new Set(files.flatMap((f) => [...gatedPersistedFields(readFileSync(f, "utf8"))]));

  it("finds the known read gates (the scan is looking in the right place)", () => {
    expect(gated.has("hasGrant")).toBe(true);
    expect(gated.has("bondedStake")).toBe(true);
  });

  it("every gating persisted field is covered", () => {
    const missing = [...gated].filter((f) => !(f in RESTART_COVERAGE));
    expect(missing, "add a null-after-restore test and list it in RESTART_COVERAGE").toEqual([]);
  });

  it("every named restart test exists", () => {
    const tests = sources("src").length ? readdirSync("src", { recursive: true }) : [];
    const testText = (tests as string[])
      .filter((p) => /\.test\.tsx?$/.test(p))
      .map((p) => readFileSync(join("src", p), "utf8"))
      .join("\n");
    for (const [f, c] of Object.entries(RESTART_COVERAGE)) {
      if ("test" in c) expect(testText.includes(c.test), `${f}: "${c.test}"`).toBe(true);
    }
  });
});
