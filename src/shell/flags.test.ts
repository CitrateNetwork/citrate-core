// L0-WP09: src/shell/flags.gen.ts is generated from release/flags.json (scripts/gen-flags.mjs), so
// the webview and the Rust consts read one table. cargo test ties the table to the consts
// (src-tauri/src/flags_tests.rs); this file ties the generated module to the table.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { OUT, render, TABLE } from "../../scripts/gen-flags.mjs";
import { ROOT } from "../../scripts/seams-snapshot.mjs";
import { FLAG_ROWS, FLAGS } from "./flags.gen";

const read = (rel: string) => readFileSync(join(ROOT, rel), "utf8");

const good = {
  $comment: ["t"],
  flags: [
    {
      name: "ALPHA",
      file: "src-tauri/src/alpha.rs",
      lane: "l1",
      default: false,
      signoff: { id: "ALPHA", packet_repo: "CitrateNetwork/citrate-core" },
      gate: null,
      summary: "turns alpha on",
    },
  ],
};

describe("flags.gen.ts", () => {
  it("flags_gen_is_current", () => {
    expect(read(OUT)).toBe(render(read(TABLE)));
  });

  it("holds each row's value and sign-off, and no file paths", () => {
    const table = JSON.parse(read(TABLE));
    expect(FLAG_ROWS.map((r) => r.name)).toEqual(table.flags.map((r: { name: string }) => r.name));
    for (const r of table.flags) {
      expect(FLAGS[r.name as keyof typeof FLAGS]).toBe(r.default);
      expect(read(OUT)).not.toContain(r.file);
      if (r.signoff) expect(read(OUT)).not.toContain(r.signoff.packet_repo);
    }
  });

  it("keeps today's values (a sign-off flips them, never this table alone)", () => {
    expect(FLAGS.HERMES_NODE_WRITE_TOOLS).toBe(false);
    expect(FLAGS.TRANSPORT_SIGNED_OFF).toBe(false);
    expect(FLAGS.MULTI_GROUP_DAEMON).toBe(true);
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
    expect(bad((r) => (r.name = "alpha"))).toThrow(/SCREAMING_CASE/);
    expect(bad((r) => (r.name = "A: true, B"))).toThrow(/SCREAMING_CASE/);
    expect(bad((r) => (r.lane = "l10"))).toThrow(/lane/);
    expect(bad((r) => (r.summary = " "))).toThrow(/summary/);
    expect(bad((r) => (r.signoff = { id: "" }))).toThrow(/signoff/);
    const dup = { ...good, flags: [good.flags[0], good.flags[0]] };
    expect(() => render(JSON.stringify(dup))).toThrow(/twice/);
    expect(() => render("{}")).toThrow(/flags/);
  });
});
