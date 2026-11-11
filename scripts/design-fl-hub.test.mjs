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

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const HUB = path.join(ROOT, "design", "fl-hub");
const DIRECTIONS = ["direction-a.html", "direction-b.html", "direction-c.html"];
const read = (p) => readFileSync(p, "utf8");

function walk(dir, out = []) {
  for (const e of readdirSync(dir)) {
    const p = path.join(dir, e);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (/\.(tsx?|css|html|json)$/.test(e) && !/\.test\.tsx?$/.test(e)) out.push(p);
  }
  return out;
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
  });

  it("the shared stylesheet and the index carry no colour literal either", () => {
    for (const f of ["hub.css", "index.html"]) {
      const p = path.join(HUB, f);
      const src = existsSync(p) ? read(p).replace(/\/\*[\s\S]*?\*\//g, "") : "missing";
      expect(src.match(/#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3}\b(?![\w-])|rgba?\(|hsla?\(|missing/g) ?? [], f).toEqual([]);
    }
  });

  it("no production module under src/ imports from design/", () => {
    const hits = walk(path.join(ROOT, "src")).filter((f) => /(from\s+|import\s*\(|url\()\s*["'][^"']*design\//.test(read(f)));
    expect(hits.map((f) => path.relative(ROOT, f))).toEqual([]);
  });
});

describe("UX-0 · each direction page", () => {
  for (const name of DIRECTIONS) {
    const file = path.join(HUB, name);
    const html = existsSync(file) ? read(file) : "";
    const text = html
      .replace(/<style[\s\S]*?<\/style>/g, " ")
      .replace(/<script[\s\S]*?<\/script>/g, " ")
      .replace(/<[^>]+>/g, " ")
      .replace(/&nbsp;/g, " ")
      .replace(/\s+/g, " ");

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
