// Tripwire: every view that subscribes to web-settings saves (webSettingsSync.ts) must drop a stale
// initial read. Its load in a useEffect has to go through readWebSettingsUnlessSuperseded; a bare
// loadWebSettings(...) there can answer after another view's save and overwrite the newer value.
// Writers re-read inside the write lock, outside effects, and are not affected by this rule.
import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const SRC = join(__dirname, "..");

function sourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...sourceFiles(p));
    else if (/\.(ts|tsx)$/.test(name) && !/\.test\.(ts|tsx)$/.test(name)) out.push(p);
  }
  return out;
}

/** The argument text of every `useEffect(` call, by paren matching. */
function effectBodies(text: string): string[] {
  const out: string[] = [];
  let at = text.indexOf("useEffect(");
  while (at >= 0) {
    let depth = 0;
    let end = at + "useEffect".length;
    for (; end < text.length; end++) {
      if (text[end] === "(") depth++;
      else if (text[end] === ")" && --depth === 0) break;
    }
    out.push(text.slice(at, end + 1));
    at = text.indexOf("useEffect(", end);
  }
  return out;
}

/** Problems in one subscribing module: bare initial loads, or no guarded load at all. */
export function staleReadProblems(text: string): string[] {
  if (!text.includes("subscribeWebSettings(")) return [];
  const problems: string[] = [];
  for (const body of effectBodies(text)) {
    const bare = body.replace(/readWebSettingsUnlessSuperseded\(\s*\(\)\s*=>\s*loadWebSettings\(/g, "");
    if (bare.includes("loadWebSettings(")) problems.push("a useEffect calls loadWebSettings without readWebSettingsUnlessSuperseded");
  }
  if (!text.includes("readWebSettingsUnlessSuperseded(")) problems.push("subscribes but has no readWebSettingsUnlessSuperseded load");
  return problems;
}

describe("tripwire: subscribing web-settings views drop stale initial reads", () => {
  it("catches a bare initial load in a subscribing view (self-test)", () => {
    const bad = `subscribeWebSettings(() => {}); useEffect(() => { loadWebSettings(io).then(setStatus); }, []);`;
    expect(staleReadProblems(bad).length).toBeGreaterThan(0);
    const good = `subscribeWebSettings(() => {}); useEffect(() => { readWebSettingsUnlessSuperseded(() => loadWebSettings(io)).then(setStatus); }, []);`;
    expect(staleReadProblems(good)).toEqual([]);
  });

  it("holds for every module in src/", () => {
    const subscribers: string[] = [];
    const failures: string[] = [];
    for (const f of sourceFiles(SRC)) {
      const text = readFileSync(f, "utf8");
      if (!text.includes("subscribeWebSettings(") || f.endsWith("webSettingsSync.ts")) continue;
      subscribers.push(relative(SRC, f));
      for (const p of staleReadProblems(text)) failures.push(`${relative(SRC, f)}: ${p}`);
    }
    // Both views on Hermes › Controls subscribe; if this drops, the scan is not seeing them.
    expect(subscribers.sort()).toEqual(expect.arrayContaining(["surfaces/WebSearchSettings.tsx", "surfaces/hermes/ControlsPanel.tsx"]));
    expect(failures).toEqual([]);
  });
});
