// v0.5.0-rc.5 FIX 3 — dark text must never land on a dark fill (owner report: "widget text is
// black on dark green").
//
// Root cause of the report: the widget document's base CSS picked its text colour from the
// OS colour scheme (`prefers-color-scheme: light` → #1d1b18), while the dashboard that hosts
// the widgets is always the dark Instrument register. On a Mac in light mode every widget
// rendered near-black text on the dark-green panel.
//
// Structural guard, three parts:
//  1. Token pairs: every fill token has a matching on-colour token, in BOTH registers, and
//     the pair meets WCAG AA (4.5:1). The button classes are checked the same way.
//  2. Source lint: any inline style that paints a fill token as a background must take its
//     text colour from the matching on-colour token (or be a text-free decoration with an
//     explicit width and height: dots, bars, carets).
//  3. Widget documents: one fixed light-on-dark palette, no OS-scheme switch, AA on every
//     Instrument surface, and the panel hosting them pins the Instrument register.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";
import { AA, contrast, cssBlock, over, parseColor, type RGBA } from "./contrastMath";

const readStyle = (file: string): string => readFileSync(join(__dirname, "../styles", file), "utf8");

const tokensCss = readStyle("tokens.css");
const foundationCss = readStyle("foundation.css");
const foundationRoot = cssBlock(foundationCss, ":root");

const REGISTERS = [
  { name: "charter (light)", selector: ':root,\n[data-register="charter"]' },
  { name: "instrument (dark)", selector: '[data-register="instrument"] {' },
] as const;

/** Fill token → the on-colour token text on that fill must use. */
const ON: Record<string, string> = {
  "--accent": "--on-accent",
  "--citrate-green": "--on-accent",
  "--accent-deep": "--on-accent-deep",
  "--citrate-yellow": "--on-highlight",
  "--danger": "--on-danger",
};

function resolver(selector: string) {
  const reg = cssBlock(tokensCss, selector);
  const raw = (k: string): string => {
    let v = reg[k] ?? foundationRoot[k];
    if (!v) throw new Error(`token ${k} is not defined for ${selector}`);
    for (let i = 0; i < 5; i++) {
      const m = v.match(/^var\((--[\w-]+)\)$/);
      if (!m) break;
      v = reg[m[1]] ?? foundationRoot[m[1]];
      if (!v) throw new Error(`token ${m[1]} is not defined`);
    }
    return v;
  };
  const base = parseColor(raw("--srf-0"));
  const color = (k: string): RGBA => {
    const c = parseColor(raw(k));
    return c[3] < 1 ? over(c, base) : c;
  };
  return { raw, color };
}

describe("FIX 3 · on-colour tokens meet AA on their fills, both registers", () => {
  for (const r of REGISTERS) {
    const { color } = resolver(r.selector);
    for (const [fill, on] of Object.entries(ON)) {
      it(`${r.name}: ${on} on ${fill}`, () => {
        const ratio = contrast(color(on), color(fill));
        expect(ratio, `${on} on ${fill}`).toBeGreaterThanOrEqual(AA);
      });
    }
  }
});

/** The declarations of the CSS rule whose selector is exactly `sel` in tokens.css. */
function rule(sel: string): Record<string, string> {
  const re = new RegExp(`(^|\\n)${sel.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*\\{([^}]*)\\}`);
  const m = tokensCss.replace(/\/\*[\s\S]*?\*\//g, "").match(re);
  if (!m) throw new Error(`no rule ${sel}`);
  const out: Record<string, string> = {};
  for (const d of m[2].matchAll(/([\w-]+)\s*:\s*([^;]+);/g)) out[d[1]] = d[2].trim();
  return out;
}

describe("FIX 3 · every button variant sets an AA text colour on its own background, both registers", () => {
  const BUTTONS = [".btn", ".btn-primary", ".btn-primary:hover", ".btn-secondary", ".btn-ghost", ".btn-ghost:hover", ".btn-danger", ".btn-danger:hover"];
  for (const r of REGISTERS) {
    const { color } = resolver(r.selector);
    // Resolve a declaration value to a colour: a var() token, `transparent` (the button sits on
    // its surface; the darkest/lightest surfaces are both checked), or a literal.
    const resolve = (v: string): RGBA[] => {
      const t = v.match(/^var\((--[\w-]+)\)$/);
      if (t) return [color(t[1])];
      if (v === "transparent") return ["--srf-0", "--srf-1", "--srf-2"].map(color);
      return [parseColor(v)];
    };
    for (const sel of BUTTONS) {
      it(`${r.name}: ${sel}`, () => {
        const base = rule(".btn");
        const own = rule(sel.split(":")[0]);
        const d = { ...base, ...own, ...(sel.includes(":") ? rule(sel) : {}) };
        expect(d.color, `${sel} must set a text colour (else the UA's buttontext black applies)`).toBeTruthy();
        expect(d.background, `${sel} must set a background (else the UA's buttonface applies)`).toBeTruthy();
        expect(d.color, `${sel}: text colour must be a token`).toMatch(/^var\(--/);
        for (const bg of resolve(d.background)) {
          for (const fg of resolve(d.color)) {
            expect(contrast(fg, bg), `${sel} ${d.color} on ${d.background}`).toBeGreaterThanOrEqual(AA);
          }
        }
      });
    }
  }
  it("no register-specific override is needed to keep primary hover legible", () => {
    expect(tokensCss).not.toMatch(/\[data-register="charter"\]\s*\.btn-primary:hover/);
  });
});

// ------------------------------------------------------------------ source lint

function walk(dir: string, out: string[] = []): string[] {
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (/\.tsx$/.test(e) && !/\.test\.tsx$/.test(e)) out.push(p);
  }
  return out;
}

/** Every inline `style={{ ... }}` object literal in `src`, with its line number. */
function inlineStyles(src: string): { line: number; body: string }[] {
  const out: { line: number; body: string }[] = [];
  let i = 0;
  while ((i = src.indexOf("style={{", i)) >= 0) {
    let depth = 0;
    let j = i + 7;
    for (; j < src.length; j++) {
      if (src[j] === "{") depth++;
      else if (src[j] === "}" && --depth === 0) break;
    }
    out.push({ line: src.slice(0, i).split("\n").length, body: src.slice(i + 8, j) });
    i = j;
  }
  return out;
}

const FILL = /background(?:Color)?:\s*"var\((--accent|--accent-deep|--citrate-green|--citrate-yellow|--danger)\)"/;

/** The violations the lint finds in one file's source (exported shape for the self-test). */
function lintFills(src: string): string[] {
  const bad: string[] = [];
  for (const { line, body } of inlineStyles(src)) {
    const f = body.match(FILL);
    if (!f) continue;
    const want = ON[f[1]];
    const c = body.match(/(?:^|[\s,{])color:\s*("[^"]*"|[^,}]+)/);
    if (c) {
      if (c[1] !== `"var(${want})"`) bad.push(`:${line} text on ${f[1]} uses ${c[1]}, not var(${want})`);
    } else if (!(/\bwidth:/.test(body) && /\bheight:/.test(body))) {
      bad.push(`:${line} ${f[1]} fill sets no text colour and is not a sized decoration`);
    }
  }
  return bad;
}

describe("FIX 3 · lint: text on a fill uses the fill's on-colour token", () => {
  it("self-test: the lint catches black-on-green and passes the token form", () => {
    expect(lintFills(`<span style={{ width: 28, height: 28, background: "var(--citrate-green)", color: "#0e0f0c" }}>A</span>`)).toHaveLength(1);
    expect(lintFills(`<b style={{ background: "var(--danger)", color: "#fff" }}>x</b>`)).toHaveLength(1);
    expect(lintFills(`<b style={{ background: "var(--accent)" }}>text</b>`)).toHaveLength(1);
    expect(lintFills(`<span style={{ width: 6, height: 6, background: "var(--accent)" }}></span>`)).toHaveLength(0);
    expect(lintFills(`<b style={{ background: "var(--accent-deep)", color: "var(--on-accent-deep)" }}>x</b>`)).toHaveLength(0);
  });

  it("no component in src/ violates it", () => {
    const root = join(__dirname, "..");
    const bad: string[] = [];
    for (const f of walk(root)) {
      for (const v of lintFills(readFileSync(f, "utf8"))) bad.push(relative(root, f) + v);
    }
    expect(bad).toEqual([]);
  });
});

// ------------------------------------------------------------------ widget documents

describe("FIX 3 · widget documents (the reported black-on-dark-green)", () => {
  const rs = readFileSync(join(__dirname, "../../src-tauri/src/widgets.rs"), "utf8");
  const m = rs.match(/const BASE_CSS: &str = "([^"]*)";/);
  const baseCss = m ? m[1] : "";
  const inst = resolver('[data-register="instrument"] {');
  const surfaces = ["--srf-0", "--srf-1", "--srf-2"].map(inst.color);

  it("the base CSS is found", () => {
    expect(baseCss.length).toBeGreaterThan(0);
  });

  it("does not switch the text colour on the OS colour scheme (the host panel is always dark)", () => {
    expect(baseCss).not.toMatch(/prefers-color-scheme/);
    expect(baseCss).toMatch(/color-scheme:\s*dark/);
  });

  it("body text meets AA on every Instrument surface", () => {
    const c = baseCss.match(/body\{[^}]*?(?:^|[;{])color:(#[0-9a-f]{3,6})/i);
    expect(c, "body sets a literal text colour").toBeTruthy();
    const fg = parseColor(c![1]);
    for (const s of surfaces) expect(contrast(fg, s)).toBeGreaterThanOrEqual(AA);
  });

  it("the gallery's caption and error styles meet AA on every Instrument surface", () => {
    const gallery = readFileSync(join(__dirname, "../widgets/gallery.ts"), "utf8");
    const style = gallery.match(/const STYLE = `([^`]*)`/)?.[1] ?? "";
    const op = Number(style.match(/\.s\{[^}]*opacity:([\d.]+)/)?.[1] ?? "1");
    const body = parseColor(baseCss.match(/body\{[^}]*?(?:^|[;{])color:(#[0-9a-f]{3,6})/i)![1]);
    const err = parseColor(style.match(/\.e\{[^}]*color:(#[0-9a-f]{6})/)![1]);
    expect(style).not.toMatch(/\.(v|s)\{[^}]*color:/); // captions inherit the base colour
    for (const s of surfaces) {
      expect(contrast(over([body[0], body[1], body[2], op], s), s), "caption").toBeGreaterThanOrEqual(AA);
      expect(contrast(err, s), "error").toBeGreaterThanOrEqual(AA);
    }
  });

  it("the widgets panel pins the Instrument register, so the host surface matches the document palette", () => {
    const panel = readFileSync(join(__dirname, "../widgets/WidgetsPanel.tsx"), "utf8");
    const roots = panel.match(/data-testid="widgets-panel"[^>]*>/g) ?? [];
    expect(roots.length).toBeGreaterThan(0);
    for (const r of roots) expect(r).toMatch(/data-register="instrument"/);
  });
});
