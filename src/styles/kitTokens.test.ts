// V60 S0-UX (UX-0): the Citrate kit tokens mapped onto the app's two registers (planset v060
// 03 §1), and the FL/mesh tokens that the FL hub, the round ring and the cluster map draw with.
//
// Contracts:
//  1. foundation.css is the kit's colors_and_type.css token for token. The only differences are
//     the self-hosted font family name ("Geist Sans" for the kit's "Geist", BC-7.1).
//  2. design/KIT_TOKEN_MAP.md is the human-readable map. Every row is checked here: the kit value
//     is the kit's, and each register's app token resolves to the value the row states. A row
//     whose app value differs from the kit value carries a note saying why.
//  3. The FL/mesh tokens exist in BOTH register blocks (a var() in a custom property resolves where
//     it is declared, so a :root-only definition would keep charter colours inside an instrument
//     subtree), name other tokens only (one colour source, Rule 9), and every state colour meets
//     3:1 against each surface of its register (WCAG 1.4.11, non-text contrast).
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { AA, contrast, cssBlock, over, parseColor, type RGBA } from "../a11y/contrastMath";

const ROOT = join(__dirname, "../..");
const read = (p: string) => readFileSync(join(ROOT, p), "utf8");
const KIT_CSS = read("design/_ds/citrate-design-system-244c4cdf-0fbf-406e-9d19-bc9db7d7b48a/colors_and_type.css");
const FOUNDATION_CSS = read("src/styles/foundation.css");
const TOKENS_CSS = read("src/styles/tokens.css");

const kitRoot = cssBlock(KIT_CSS, ":root {");
const foundationRoot = cssBlock(FOUNDATION_CSS, ":root {");
const zoneRoot = cssBlock(TOKENS_CSS, ":root {");

const REGISTERS = {
  charter: ':root,\n[data-register="charter"]',
  instrument: '[data-register="instrument"] {',
} as const;
type Register = keyof typeof REGISTERS;

/** Resolve a token in a register: the register block, then the zone :root, then the foundation. */
function resolver(register: Register) {
  const reg = cssBlock(TOKENS_CSS, REGISTERS[register]);
  const lookup = (k: string) => reg[k] ?? zoneRoot[k] ?? foundationRoot[k];
  const raw = (k: string): string => {
    let v = lookup(k);
    if (!v) throw new Error(`token ${k} is not defined for ${register}`);
    for (let i = 0; i < 6; i++) {
      const m = v.match(/^var\((--[\w-]+)\)$/);
      if (!m) break;
      const next = lookup(m[1]);
      if (!next) throw new Error(`token ${m[1]} (from ${k}) is not defined for ${register}`);
      v = next;
    }
    return v;
  };
  const color = (k: string, on = "--srf-0"): RGBA => {
    const c = parseColor(raw(k));
    return c[3] < 1 ? over(c, parseColor(raw(on))) : c;
  };
  return { reg, raw, color };
}

const hex = (c: RGBA) => "#" + c.slice(0, 3).map((v) => Math.round(v).toString(16).padStart(2, "0")).join("");

describe("UX-0 · foundation.css is the kit's token set", () => {
  const FONT_FAMILY_SWAP = new Set(["--font-display", "--font-sans"]);

  it("declares every kit token with the kit's value (fonts: self-hosted family name only)", () => {
    const keys = Object.keys(kitRoot);
    expect(keys.length).toBeGreaterThan(80);
    for (const k of keys) {
      expect(foundationRoot[k], `${k} missing from foundation.css`).toBeDefined();
      if (FONT_FAMILY_SWAP.has(k)) {
        expect(foundationRoot[k].replace(/"Geist Sans"/g, '"Geist"'), k).toBe(kitRoot[k]);
      } else {
        expect(foundationRoot[k], k).toBe(kitRoot[k]);
      }
    }
  });

  it("keeps the 03 §1 brand base: 8 pt spacing and motion 140/220/420/900 ms", () => {
    expect(foundationRoot["--s-2"]).toBe("8px");
    expect(["--dur-fast", "--dur-base", "--dur-slow", "--dur-narrative"].map((k) => foundationRoot[k])).toEqual([
      "140ms",
      "220ms",
      "420ms",
      "900ms",
    ]);
  });
});

// ------------------------------------------------------------------ the map document

type Row = { kit: string; kitValue: string; charter?: [string, string]; instrument?: [string, string]; note: string };

const MAP = "design/KIT_TOKEN_MAP.md";

function mapRows(): Row[] {
  if (!existsSync(join(ROOT, MAP))) return [];
  const md = read(MAP);
  const rows: Row[] = [];
  const cell = (s: string): [string, string] | undefined => {
    if (/^none\b/.test(s.trim())) return undefined;
    const m = s.match(/`(--[\w-]+)`\s*=\s*`([^`]+)`/);
    if (!m) throw new Error(`cannot read map cell "${s}"`);
    return [m[1], m[2]];
  };
  for (const line of md.split("\n")) {
    const m = line.match(/^\|\s*`(--[\w-]+)`\s*\|\s*`([^`]+)`\s*\|([^|]*)\|([^|]*)\|([^|]*)\|\s*$/);
    if (!m) continue;
    rows.push({ kit: m[1], kitValue: m[2], charter: cell(m[3]), instrument: cell(m[4]), note: m[5].trim() });
  }
  return rows;
}

describe("UX-0 · design/KIT_TOKEN_MAP.md matches the CSS", () => {
  const rows = mapRows();
  const res = { charter: resolver("charter"), instrument: resolver("instrument") };

  it("exists and has rows", () => {
    expect(existsSync(join(ROOT, MAP)), MAP).toBe(true);
    expect(rows.length).toBeGreaterThan(10);
  });

  it("maps every brand colour 03 §1 names", () => {
    const named = [
      "--citrate-green",
      "--citrate-green-deep",
      "--citrate-yellow",
      "--ink",
      "--paper",
      "--deep-evergreen",
      "--semantic-success",
      "--semantic-warning",
      "--semantic-danger",
      "--semantic-info",
    ];
    const kits = rows.map((r) => r.kit);
    for (const k of named) expect(kits, k).toContain(k);
  });

  for (const r of rows) {
    it(`${r.kit}: kit value and each register's app token`, () => {
      expect(kitRoot[r.kit], `${r.kit} is not a kit token`).toBeDefined();
      expect(kitRoot[r.kit].toLowerCase()).toBe(r.kitValue.toLowerCase());
      expect(r.charter ?? r.instrument, `${r.kit} maps to nothing in either register`).toBeDefined();
      for (const reg of ["charter", "instrument"] as const) {
        const m = r[reg];
        if (!m) continue;
        const [token, value] = m;
        expect(res[reg].raw(token).toLowerCase(), `${reg} ${token}`).toBe(value.toLowerCase());
        if (value.toLowerCase() !== r.kitValue.toLowerCase()) {
          expect(r.note.length, `${r.kit} → ${reg} ${token} differs from the kit and needs a note`).toBeGreaterThan(0);
        }
      }
    });
  }
});

// ------------------------------------------------------------------ FL and mesh tokens

/** State colours: each must read at 3:1 on every surface of its register. */
const FL_STATE = [
  "--fl-ring-done",
  "--fl-ring-current",
  "--fl-ring-disputed",
  "--fl-ring-expired",
  "--fl-lease-leased",
  "--fl-lease-submitted",
  "--fl-lease-verified",
  "--fl-lease-rejected",
  "--fl-escrow-paid",
  "--fl-escrow-reserved",
  "--fl-escrow-refundable",
  "--mesh-edge-direct",
  "--mesh-edge-relayed",
];
/** Decoration and band tokens: defined in both registers, checked separately. */
const FL_OTHER = ["--fl-ring-track", "--fl-band", "--fl-on-band"];
/** Groups whose members must be told apart by colour as well as by label or shape. */
const DISTINCT = [
  ["--fl-ring-done", "--fl-ring-current", "--fl-ring-disputed", "--fl-ring-expired"],
  ["--fl-lease-leased", "--fl-lease-submitted", "--fl-lease-verified", "--fl-lease-rejected"],
  ["--fl-escrow-paid", "--fl-escrow-reserved", "--fl-escrow-refundable"],
  ["--mesh-edge-direct", "--mesh-edge-relayed"],
];
const NON_TEXT = 3;

describe("UX-0 · FL and mesh tokens, both registers", () => {
  for (const register of ["charter", "instrument"] as const) {
    const { reg, color } = resolver(register);

    it(`${register}: every FL/mesh token is declared in the register block and names a token`, () => {
      for (const t of [...FL_STATE, ...FL_OTHER]) {
        expect(reg[t], `${t} not declared in the ${register} block`).toBeDefined();
        expect(reg[t], `${t} must be var(--token), not a literal colour`).toMatch(/^var\(--[\w-]+\)$/);
      }
    });

    for (const t of FL_STATE) {
      it(`${register}: ${t} reads at ${NON_TEXT}:1 on --srf-0, --srf-1 and --srf-2`, () => {
        for (const s of ["--srf-0", "--srf-1", "--srf-2"]) {
          const ratio = contrast(color(t, s), color(s));
          expect(ratio, `${t} on ${s} (${register})`).toBeGreaterThanOrEqual(NON_TEXT);
        }
      });
    }

    it(`${register}: --fl-on-band text meets AA on --fl-band`, () => {
      expect(contrast(color("--fl-on-band"), color("--fl-band"))).toBeGreaterThanOrEqual(AA);
    });

    it(`${register}: states within a group are different colours`, () => {
      for (const group of DISTINCT) {
        const seen = group.map((t) => hex(color(t)));
        expect(new Set(seen).size, `${group.join(", ")} → ${seen.join(", ")}`).toBe(group.length);
      }
    });
  }
});
