// created: 2026-11-28 | branch: feat/v60-l0-wp09-flags
// author: Larry Klosowski (L0 lane, L0-WP09; Claude Opus 5.5)
// status: active
//
// L0-WP09: src/shell/flags.gen.ts is generated from release/flags.json (scripts/gen-flags.mjs), so
// the webview and the Rust consts read one table, and src/shell/flags.ts exports flag(id) over it
// (L0_INTEGRATION_SEAMS 3.7). cargo test ties the table to the consts
// (src-tauri/src/flags_tests.rs); this file ties the generated module to the table.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { OUT, render, TABLE } from "../../scripts/gen-flags.mjs";
import { ROOT } from "../../scripts/seams-snapshot.mjs";
import { flag, type FlagId } from "./flags";
import { FLAG_ROWS, FLAGS } from "./flags.gen";

const read = (rel: string) => readFileSync(join(ROOT, rel), "utf8");

const good = {
  version: 1,
  flags: [
    {
      id: "ALPHA",
      lane: "L1",
      where: "src-tauri/src/alpha.rs",
      default: false,
      signoff: "required",
      record: "release/signoffs/ALPHA.json",
    },
  ],
};

describe("flags.gen.ts", () => {
  it("flags_gen_is_current", () => {
    expect(read(OUT)).toBe(render(read(TABLE)));
  });

  it("holds each row's value and sign-off, and no repo paths", () => {
    const table = JSON.parse(read(TABLE));
    expect(FLAG_ROWS.map((r) => r.id)).toEqual(table.flags.map((r: { id: string }) => r.id));
    for (const r of table.flags) {
      expect(FLAGS[r.id as keyof typeof FLAGS]).toBe(r.default);
      expect(flag(r.id as FlagId)).toBe(r.default);
      if (r.where !== TABLE) expect(read(OUT)).not.toContain(r.where);
      if (r.record) expect(read(OUT)).not.toContain(r.record);
    }
  });

  it("keeps today's values (a sign-off flips them, never this table alone)", () => {
    expect(flag("HERMES_NODE_WRITE_TOOLS")).toBe(false);
    expect(flag("TRANSPORT_SIGNED_OFF")).toBe(false);
    expect(flag("MULTI_GROUP_DAEMON")).toBe(true);
    expect(flag("PQC_CLAIMS_ALLOWED")).toBe(false);
  });

  it("flag() reads an id the table does not hold as off", () => {
    for (const id of ["NOT_A_FLAG", "__proto__", "constructor", "toString", "hasOwnProperty"]) {
      expect(flag(id as FlagId)).toBe(false);
    }
  });

  it("the owner approves every change to the table", () => {
    // Dropping a row's signoff and flipping the const in one PR would pass cargo test, so the
    // table is owner-pinned after every lane rule (last match wins in CODEOWNERS).
    const rules = read(".github/CODEOWNERS").split("\n").filter((l) => l.trim() && !l.startsWith("#"));
    const at = rules.indexOf(`/${TABLE} @SaulBuilds`);
    expect(at).toBeGreaterThan(rules.findLastIndex((l) => l.endsWith(" @CitrateNetwork/maintainers")));
    expect(rules.slice(at + 1).some((l) => l.split(/\s+/)[0] === `/${TABLE}`)).toBe(false);
  });

  it("refuses a table it cannot render faithfully", () => {
    expect(() => render(JSON.stringify(good))).not.toThrow();
    const bad = (edit: (r: Record<string, unknown>) => void) => {
      const t = structuredClone(good) as { flags: Record<string, unknown>[] };
      edit(t.flags[0]);
      return () => render(JSON.stringify(t));
    };
    expect(bad((r) => (r.default = "false"))).toThrow(/true or false/);
    expect(bad((r) => (r.id = "alpha"))).toThrow(/SCREAMING_CASE/);
    expect(bad((r) => (r.id = "A: true, B"))).toThrow(/SCREAMING_CASE/);
    expect(bad((r) => (r.lane = "l1"))).toThrow(/lane/);
    expect(bad((r) => (r.lane = "L10"))).toThrow(/lane/);
    expect(bad((r) => (r.signoff = { id: "ALPHA" }))).toThrow(/signoff/);
    expect(bad((r) => (r.signoff = "maybe"))).toThrow(/signoff/);
    const dup = { ...good, flags: [good.flags[0], good.flags[0]] };
    expect(() => render(JSON.stringify(dup))).toThrow(/twice/);
    expect(() => render(JSON.stringify({ flags: [] }))).toThrow(/version/);
    expect(() => render(JSON.stringify({ version: 1 }))).toThrow(/flags/);
  });
});
