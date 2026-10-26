// K4 — storage wording/formatting helpers (pure).
import { describe, it, expect } from "vitest";
import { formatSaltWei, pinStateLabel, progressPct, progressText, describeMismatch, LOCAL_PIN_LABEL } from "./storageTruth";

describe("K4 storageTruth", () => {
  it("formats wei as exact SALT; unknown is a dash, never 0", () => {
    expect(formatSaltWei("10000000000000000000")).toBe("10 SALT");
    expect(formatSaltWei("4000000000000000000")).toBe("4 SALT");
    expect(formatSaltWei("1500000000000000000")).toBe("1.5 SALT");
    expect(formatSaltWei("32000000000000000000000")).toBe("32,000 SALT");
    expect(formatSaltWei(null)).toBe("—");
    expect(formatSaltWei(undefined)).toBe("—");
    expect(formatSaltWei("not-a-number")).toBe("—");
  });

  it("a pin is a local pin or not pinned; reserved chain states are never labelled as live", () => {
    expect(pinStateLabel({ pinState: "pinned" })).toBe(LOCAL_PIN_LABEL);
    expect(LOCAL_PIN_LABEL).toBe("Local pin · no bond");
    for (const s of ["unpinned", "pinning", "challenged"] as const) {
      expect(pinStateLabel({ pinState: s })).toBe("Not pinned on this node");
    }
  });

  it("progress: percentage only when there is a count", () => {
    expect(progressPct(null)).toBeNull();
    expect(progressPct({ op: "export", phase: "verify", done: 0, total: 0, item: "" })).toBeNull();
    expect(progressPct({ op: "restore", phase: "memory", done: 3, total: 4, item: "" })).toBe(75);
    expect(progressText({ op: "restore", phase: "memory", done: 3, total: 4, item: "" })).toBe("Memory 3 of 4");
    expect(progressText(null)).toBe("Starting…");
  });

  it("describes every mismatch kind", () => {
    expect(describeMismatch({ kind: "sizeDiffers", path: "memory/x", expected: 16, actual: 1 })).toBe("memory/x: 1 bytes, the manifest says 16");
    expect(describeMismatch({ kind: "notAFile", path: "memory/l" })).toContain("not a regular file");
    expect(describeMismatch({ kind: "unsafePath", path: "../x" })).toContain("not an allowed backup path");
    expect(describeMismatch({ kind: "modeDiffers", path: "memory/a", expected: 0o600, actual: 0o644 })).toBe(
      "memory/a: mode 644 after restore, the manifest says 600",
    );
  });
});
