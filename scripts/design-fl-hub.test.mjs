// V60 S0-UX (UX-0): the three FL hub directions are design mocks. They hold fixture figures, so
// they must never reach the shipped app (Rule 1), and they must draw with the app's own tokens so
// the direction the owner picks is the one UX-1 builds (Rule 9).
//
// Contracts:
//  1. design/fl-hub/ has an index and the three direction pages.
//  2. Nothing the production build reads points at design/: the Vite entry (index.html), the Vite
//     config, and every module under src/.
//  3. Each page says it is a design mock with fixture data, links foundation.css and tokens.css
//     from src/styles, and contains no colour literal of its own.
//  4. Each page draws the operations 03 §2 lists, the three explicit empty and error states, and
//     the tokenomics notice; and follows the copy rules (SALT only, no USD, no projections,
//     no em-dashes).
import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { JSDOM } from "jsdom";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const HUB = path.join(ROOT, "design", "fl-hub");
const DIRECTIONS = ["direction-a.html", "direction-b.html", "direction-c.html"];
const read = (p) => readFileSync(p, "utf8");

/** Every source the production build can read: TS/JS in every form, CSS, HTML, JSON. */
const SOURCE = /\.(tsx?|jsx?|mjs|cjs|css|html|json)$/;
const TEST_FILE = /\.test\.(tsx?|jsx?|mjs|cjs)$/;

function walk(dir, out = []) {
  for (const e of readdirSync(dir)) {
    const p = path.join(dir, e);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (SOURCE.test(e) && !TEST_FILE.test(e)) out.push(p);
  }
  return out;
}

/**
 * Whether `src` points at the design/ folder by any path form a build follows: a quoted module
 * specifier or string (`from "…"`, `import "…"`, `import("…")`, `new URL("…")`,
 * `import.meta.glob("…")`, CSS `@import "…"`) or an unquoted CSS `url(…)`, with design/ as any
 * path segment. Prose in parentheses, "(design/…)", is not a reference.
 */
function pointsAtDesign(src) {
  return /(?:["'`]|\b(?:url|import|glob|URL)\(\s*["'`]?)\s*(?:\.{1,2}\/)*(?:[\w.@-]+\/)*design\//.test(src);
}

/** The page's visible text, from the parsed document (no regex HTML stripping). */
function visibleText(html) {
  const doc = new JSDOM(html).window.document;
  for (const n of doc.querySelectorAll("script, style, template, noscript")) n.remove();
  return (doc.body?.textContent ?? "").replace(/\s+/g, " ");
}

describe("UX-0 · FL hub mocks live only under design/", () => {
  it("has the index and three direction pages", () => {
    for (const f of ["index.html", ...DIRECTIONS]) expect(existsSync(path.join(HUB, f)), f).toBe(true);
  });

  it("the Vite entry and config do not reference design/", () => {
    expect(read(path.join(ROOT, "index.html"))).not.toMatch(/design\//);
    const vite = read(path.join(ROOT, "vite.config.ts"));
    expect(vite).not.toMatch(/design\//);
    expect(vite, "a second build input could pull a mock page into dist").not.toMatch(/rollupOptions\s*:\s*\{[^}]*input/);
    expect(vite, "design/ as Vite's public dir would copy the mocks into dist").not.toMatch(/publicDir\s*:\s*["'`](?:\.{1,2}\/)*design\b/);
  });

  it("the Tauri bundle resources do not include design/", () => {
    const conf = JSON.parse(read(path.join(ROOT, "src-tauri", "tauri.conf.json")));
    const res = conf.bundle?.resources ?? [];
    const entries = Array.isArray(res) ? res : [...Object.keys(res), ...Object.values(res)];
    expect(entries.filter((r) => /(^|[\\/])design([\\/]|$)/.test(String(r)))).toEqual([]);
  });

  it("the design/ matcher catches every path form a build follows", () => {
    for (const s of [
      'import x from "../design/fl-hub/a.js"',
      'import "../../design/fl-hub/hub.css"',
      "const m = await import('../design/x.mjs')",
      'new URL("../design/fl-hub/a.png", import.meta.url)',
      'import.meta.glob("../design/**/*.html")',
      '@import "../design/fl-hub/hub.css";',
      "background: url(../design/fl-hub/screens/a.png)",
      "export * from `design/fl-hub/x.js`",
    ]) {
      expect(pointsAtDesign(s), s).toBe(true);
    }
    for (const s of [
      'import x from "./designer/a.js"',
      'const t = "redesign/notes"',
      "// see design/ in the repo root",
      "// model (design/CitrateCore.dc.html): constructor",
      "   the design bundle (design/).",
    ]) {
      expect(pointsAtDesign(s), s).toBe(false);
    }
  });

  it("the shared stylesheet and the index carry no colour literal either", () => {
    for (const f of ["hub.css", "index.html"]) {
      const p = path.join(HUB, f);
      const src = existsSync(p) ? read(p).replace(/\/\*[\s\S]*?\*\//g, "") : "missing";
      expect(src.match(/#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3}\b(?![\w-])|rgba?\(|hsla?\(|missing/g) ?? [], f).toEqual([]);
    }
  });

  it("no production module under src/ points at design/ (TS, JS, MJS, CSS, HTML, JSON)", () => {
    const files = walk(path.join(ROOT, "src"));
    expect(files.some((f) => f.endsWith(".css")), "the walk must see the stylesheets").toBe(true);
    const hits = files.filter((f) => pointsAtDesign(read(f)));
    expect(hits.map((f) => path.relative(ROOT, f))).toEqual([]);
  });
});

describe("UX-0 · each direction page", () => {
  for (const name of DIRECTIONS) {
    const file = path.join(HUB, name);
    const html = existsSync(file) ? read(file) : "";
    const text = html ? visibleText(html) : "";

    it(`${name}: says it is a design mock with fixture data`, () => {
      expect(html).toMatch(/data-design-mock/);
      expect(text).toMatch(/Design mock/);
      expect(text).toMatch(/fixture/i);
    });

    it(`${name}: draws with the app's tokens and no colour literal of its own`, () => {
      expect(html).toMatch(/href="\.\.\/\.\.\/src\/styles\/foundation\.css"/);
      expect(html).toMatch(/href="\.\.\/\.\.\/src\/styles\/tokens\.css"/);
      expect(html.match(/#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3}\b(?![\w-])|rgba?\(|hsla?\(/g) ?? []).toEqual([]);
    });

    it(`${name}: shows the operations 03 §2 lists, each with its source`, () => {
      for (const part of ["round-ring", "lease-dots", "challenge-clock", "escrow-bar", "belnap", "my-timeline", "payout-receipt", "match-card", "join-sheet", "advanced"]) {
        expect(html, part).toMatch(new RegExp(`data-part="${part}"`));
      }
      for (const src of ["WorkMarket", "FederatedRoundLedger", "coordinator-reported"]) expect(text, src).toContain(src);
    });

    it(`${name}: carries the three explicit empty and error states`, () => {
      expect(text).toContain("No open rounds right now");
      expect(text).toMatch(/Coordinator not answering/);
      expect(text).toContain("Not available yet: contract not deployed on this network");
    });

    it(`${name}: carries the tokenomics notice and follows the copy rules`, () => {
      expect(text).toMatch(/Token economics are to be determined/);
      expect(text).not.toMatch(/\bUSD\b|\$\s?\d|\bAPY\b|\bAPR\b|projected|guaranteed|\bearn(ings|ed|s)?\b|dogfood/i);
      expect(html, "no em-dashes (writing rule)").not.toMatch(/—|&mdash;/);
    });
  }
});
