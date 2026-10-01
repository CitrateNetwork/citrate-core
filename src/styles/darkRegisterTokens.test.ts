// Bug (owner, 2026-10-01): in the dark ("instrument") register, chat paragraphs and inline code
// rendered near-black on the dark-green panel. foundation.css colours `p` and `code` with
// --fg-1 (= --ink, #0e0f0c); the instrument register redefined only the app's --tx-* tokens.
// Contract: the instrument register maps every foundation colour token onto its own palette.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const css = (f: string) => readFileSync(join(__dirname, f), "utf8");

function block(src: string, selector: string): string {
  const i = src.indexOf(selector + " {");
  if (i < 0) throw new Error(`no ${selector} block`);
  return src.slice(i, src.indexOf("}", i));
}

describe("instrument (dark) register", () => {
  const dark = block(css("tokens.css"), '[data-register="instrument"]');

  it("maps the foundation text tokens onto the light-on-dark --tx tokens", () => {
    expect(dark).toMatch(/--fg-1:\s*var\(--tx-1\)/);
    expect(dark).toMatch(/--fg-2:\s*var\(--tx-2\)/);
    expect(dark).toMatch(/--fg-3:\s*var\(--tx-3\)/);
  });

  it("maps the foundation surface and border tokens too", () => {
    for (const t of ["--bg-1", "--bg-2", "--bg-3", "--border-1", "--border-2", "--border-strong", "--fg-accent"]) {
      expect(dark, t).toMatch(new RegExp(`${t}:\\s*var\\(--`));
    }
  });

  it("covers every foundation colour token that p/code/body text depends on", () => {
    // foundation.css still colours p and code with --fg-1, so the mapping above is what fixes them.
    const f = css("foundation.css");
    expect(f).toMatch(/\.t-body, p \{[^}]*color: var\(--fg-1\)/);
    expect(f).toMatch(/code, kbd, samp, pre \{[^}]*color: var\(--fg-1\)/);
  });
});
