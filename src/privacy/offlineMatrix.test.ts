// HUP-S10.5 — the offline matrix data and its doc stay in step, and read plainly.
// The behavioural probes live in Rust (src-tauri/src/privacy_contract_tests.rs).
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import matrix from "./offline-matrix.json";
import fields from "./telemetry-fields.json";
import budgets from "./budget-defaults.json";

const doc = readFileSync(join(__dirname, "../../docs/OFFLINE_MATRIX.md"), "utf8");
const privacyDoc = readFileSync(join(__dirname, "../../docs/PRIVACY_AND_RECOVERY.md"), "utf8");

describe("offline matrix", () => {
  it("every feature id appears in docs/OFFLINE_MATRIX.md, with its probe", () => {
    for (const f of matrix.features) {
      expect(doc, f.id).toContain("`" + f.id + "`");
      if (f.probe.startsWith("rust:")) expect(doc, f.id).toContain("`" + f.probe.slice(5) + "`");
    }
  });

  it("every row has a known status and a plain behaviour sentence", () => {
    for (const f of matrix.features) {
      expect(["works", "degrades", "unavailable"]).toContain(f.offline);
      expect(f.behaviour.length).toBeGreaterThan(10);
      expect(f.probe === "none" || f.probe.startsWith("rust:")).toBe(true);
    }
  });
});

describe("privacy copy", () => {
  const texts = [
    JSON.stringify(matrix),
    JSON.stringify(fields),
    JSON.stringify(budgets),
    doc,
    privacyDoc,
  ];
  it("uses no em-dashes and none of the banned words", () => {
    for (const t of texts) {
      expect(t).not.toContain("—");
      expect(t.toLowerCase()).not.toContain("dogfood");
      expect(t).not.toContain("HITL");
    }
  });
});

describe("budget defaults", () => {
  it("are marked pending owner sign-off", () => {
    expect(budgets.status).toBe("placeholder-pending-owner-sign-off");
    expect(budgets.note).toContain("pending owner sign-off");
  });
});
