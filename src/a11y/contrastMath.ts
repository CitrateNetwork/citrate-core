// WCAG 2.x contrast maths shared by the colour-contrast tests (HUP-S10.6, rc.5 FIX 3).
// jsdom cannot compute colours, so these tests read the real token values from the CSS and
// composite alpha colours over the surface they sit on. Pure (no Node imports): the app's own
// tsconfig type-checks this file too.

export type RGBA = [number, number, number, number];

export const AA = 4.5;

export function parseColor(c: string): RGBA {
  const v = c.trim();
  const h6 = v.match(/^#([0-9a-f]{6})$/i);
  if (h6) return [0, 2, 4].map((i) => parseInt(h6[1].slice(i, i + 2), 16)).concat(1) as RGBA;
  const h3 = v.match(/^#([0-9a-f]{3})$/i);
  if (h3) return [0, 1, 2].map((i) => parseInt(h3[1][i] + h3[1][i], 16)).concat(1) as RGBA;
  const m = v.match(/^rgba?\(([^)]+)\)$/);
  if (!m) throw new Error(`cannot parse colour ${c}`);
  const p = m[1].split(",").map((x) => Number(x.trim()));
  return [p[0], p[1], p[2], p[3] ?? 1];
}

export const over = (fg: RGBA, bg: RGBA): RGBA => [0, 1, 2].map((i) => fg[i] * fg[3] + bg[i] * (1 - fg[3])).concat(1) as RGBA;

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

/** The custom properties declared in the first CSS block whose selector starts with `selector`. */
export function cssBlock(css: string, selector: string): Record<string, string> {
  const i = css.indexOf(selector);
  if (i < 0) throw new Error(`no ${selector} block`);
  const body = css.slice(css.indexOf("{", i) + 1, css.indexOf("}", i));
  const out: Record<string, string> = {};
  for (const m of body.matchAll(/(--[\w-]+):\s*([^;]+);/g)) out[m[1]] = m[2].trim();
  return out;
}
