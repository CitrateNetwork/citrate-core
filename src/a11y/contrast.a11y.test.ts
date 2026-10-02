// HUP-S10.6 — colour contrast of the register tokens, both registers (WCAG 2.2 SC 1.4.3, AA).
// jsdom cannot compute colours, so axe's colour-contrast rule is off in the DOM tests; this test
// reads the real token values from tokens.css and checks every text-on-surface pairing the
// pop-outs, approval cards and verdict cards use. Alpha colours are composited over the surface
// they sit on. Small text (all of these are under 18.66px bold / 24px) needs 4.5:1.
// Also: the reduced-motion contract (SC 2.3.3 / 2.2.2) that kills every named animation.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { ORIGIN_COLORS } from "../shell/state";

const tokensCss = readFileSync(join(__dirname, "../styles/tokens.css"), "utf8");

function block(selector: string): Record<string, string> {
  const i = tokensCss.indexOf(selector);
  if (i < 0) throw new Error(`no ${selector} block`);
  const body = tokensCss.slice(tokensCss.indexOf("{", i) + 1, tokensCss.indexOf("}", i));
  const out: Record<string, string> = {};
  for (const m of body.matchAll(/(--[\w-]+):\s*([^;]+);/g)) out[m[1]] = m[2].trim();
  return out;
}

type RGBA = [number, number, number, number];
function parse(c: string): RGBA {
  const h = c.match(/^#([0-9a-f]{6})$/i);
  if (h) return [0, 2, 4].map((i) => parseInt(h[1].slice(i, i + 2), 16)).concat(1) as RGBA;
  const m = c.match(/^rgba?\(([^)]+)\)$/);
  if (!m) throw new Error(`cannot parse colour ${c}`);
  const p = m[1].split(",").map((x) => Number(x.trim()));
  return [p[0], p[1], p[2], p[3] ?? 1];
}
const over = (fg: RGBA, bg: RGBA): RGBA => [0, 1, 2].map((i) => fg[i] * fg[3] + bg[i] * (1 - fg[3])).concat(1) as RGBA;
function luminance(c: RGBA): number {
  const [r, g, b] = c.slice(0, 3).map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
export function contrast(a: RGBA, b: RGBA): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const AA = 4.5;

// [text token, surface token]: every pairing the S10.6 surfaces render.
const PAIRS: [string, string][] = [
  ["--tx-1", "--srf-0"], ["--tx-1", "--srf-1"], ["--tx-1", "--srf-2"],
  ["--tx-2", "--srf-0"], ["--tx-2", "--srf-1"], ["--tx-2", "--srf-2"],
  ["--tx-3", "--srf-0"], ["--tx-3", "--srf-1"], ["--tx-3", "--srf-2"],
  ["--ok", "--ok-bg"], ["--ok", "--srf-1"],
  ["--warn", "--warn-bg"], ["--warn", "--srf-2"],
  ["--danger", "--danger-bg"], ["--danger", "--srf-1"],
  ["--accent-text", "--srf-0"], ["--accent-text", "--srf-1"], ["--accent-text", "--srf-2"],
  ["--tx-1", "--ok-bg"], ["--tx-1", "--warn-bg"],
];

for (const [register, selector, page] of [
  ["charter (light)", ":root,\n[data-register=\"charter\"]", "#ffffff"],
  ["instrument (dark)", '[data-register="instrument"] {', null],
] as const) {
  describe(`HUP-S10.6 contrast, ${register} register`, () => {
    const t = block(selector);
    // Opaque base under translucent surfaces: the dialog panel (white) in charter, --srf-0 in instrument.
    const base = parse(page ?? t["--srf-0"]);
    const surface = (k: string): RGBA => {
      const c = parse(t[k]);
      return c[3] < 1 ? over(c, over(parse(t["--srf-1"]), base)) : c;
    };
    for (const [fg, bg] of PAIRS) {
      it(`${fg} on ${bg} meets AA (${AA}:1)`, () => {
        expect(t[fg], fg).toBeTruthy();
        expect(t[bg], bg).toBeTruthy();
        const b = surface(bg);
        const ratio = contrast(over(parse(t[fg]), b), b);
        expect(ratio, `${fg} ${t[fg]} on ${bg} ${t[bg]}`).toBeGreaterThanOrEqual(AA);
      });
    }
  });
}

describe("HUP-S10.6 contrast, ceremony origin pills on the white dialog panel", () => {
  for (const [origin, colour] of Object.entries(ORIGIN_COLORS)) {
    it(`${origin} meets AA`, () => {
      expect(contrast(parse(colour), parse("#ffffff"))).toBeGreaterThanOrEqual(AA);
    });
  }
});

describe("HUP-S10.6 reduced motion", () => {
  const i = tokensCss.indexOf("@media (prefers-reduced-motion: reduce)");
  const rule = tokensCss.slice(i, tokensCss.indexOf("}\n}", i) + 3);
  it("one global rule stops animations and transitions for every element", () => {
    expect(i).toBeGreaterThan(-1);
    expect(rule).toMatch(/\*,\s*\*::before,\s*\*::after/);
    expect(rule).toMatch(/animation-duration:\s*\.01ms !important/);
    expect(rule).toMatch(/animation-iteration-count:\s*1 !important/);
    expect(rule).toMatch(/transition-duration:\s*\.01ms !important/);
  });
  it("the S10.6 surfaces animate only through the named classes that rule covers (no inline motion)", () => {
    for (const f of ["popout/ActivityMonitor.tsx", "popout/PopoutRoot.tsx", "shell/ApprovalCardView.tsx", "shell/DeployGateCard.tsx", "shell/ModalDialog.tsx"]) {
      const src = readFileSync(join(__dirname, "..", f), "utf8");
      expect(src, f).not.toMatch(/\banimation\s*:/);
      expect(src, f).not.toMatch(/\btransition\s*:/);
      expect(src, f).not.toMatch(/requestAnimationFrame/);
    }
  });
});
