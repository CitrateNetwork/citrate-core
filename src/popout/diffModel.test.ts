// HUP-S5.4 — the Code and diff pop-out's data: what crosses into the window is checked, and the
// line diff is exact (longest common subsequence) with honest numbering.
import { describe, expect, it } from "vitest";
import { describeSide, diffStats, hunks, lineDiff, parseSide, parseStepDiff, splitLines, MAX_COMPARE_CELLS, MAX_DIFF_FILES } from "./diffModel";

const ok = (files: unknown[] = []) => ({ ok: true, session: "s3-ab", seq: 2, status: "committed", files, kind: null, reason: null });

describe("diffModel parsing", () => {
  it("accepts every side kind the sidecar sends", () => {
    expect(parseSide({ kind: "absent" })).toEqual({ kind: "absent" });
    expect(parseSide({ kind: "text", text: "a\n" })).toEqual({ kind: "text", text: "a\n" });
    expect(parseSide({ kind: "binary", size: 3 })).toEqual({ kind: "binary", size: 3 });
    expect(parseSide({ kind: "too_large", size: 900000 })).toEqual({ kind: "too_large", size: 900000 });
    expect(parseSide({ kind: "symlink", target: "../x" })).toEqual({ kind: "symlink", target: "../x" });
    expect(parseSide({ kind: "unavailable", reason: "undone" })).toEqual({ kind: "unavailable", reason: "undone" });
  });

  it("refuses malformed sides and diffs", () => {
    for (const bad of [null, [], { kind: "text" }, { kind: "binary", size: -1 }, { kind: "binary", size: 1.5 }, { kind: "exec", cmd: "rm" }, { kind: "text", text: "x".repeat(256 * 1024 + 1) }]) {
      expect(parseSide(bad)).toBeNull();
    }
    expect(parseStepDiff(ok())).not.toBeNull();
    expect(parseStepDiff({ ...ok(), session: "../etc" })).toBeNull();
    expect(parseStepDiff({ ...ok(), seq: 0 })).toBeNull();
    expect(parseStepDiff({ ...ok(), status: "applied" })).toBeNull();
    expect(parseStepDiff({ ...ok(), files: [{ path: "", before: { kind: "absent" }, after: { kind: "absent" } }] })).toBeNull();
    expect(parseStepDiff({ ...ok(), files: [{ path: "a", before: { kind: "absent" } }] })).toBeNull();
    expect(parseStepDiff({ ...ok(), files: new Array(MAX_DIFF_FILES + 1).fill({ path: "a", before: { kind: "absent" }, after: { kind: "absent" } }) })).toBeNull();
    expect(parseStepDiff({ ...ok(), kind: 7 })).toBeNull();
    const refused = parseStepDiff({ ok: false, session: "s3-ab", seq: 1, status: "", files: [], kind: "pruned", reason: "step 1 was pruned" });
    expect(refused?.reason).toBe("step 1 was pruned");
  });
});

describe("lineDiff", () => {
  it("splits lines without inventing a last empty one", () => {
    expect(splitLines("")).toEqual([]);
    expect(splitLines("a\nb\n")).toEqual(["a", "b"]);
    expect(splitLines("a\nb")).toEqual(["a", "b"]);
  });

  it("finds the changed line and numbers both sides", () => {
    const { lines, exact } = lineDiff("one\ntwo\nthree\n", "one\n2\nthree\n");
    expect(exact).toBe(true);
    expect(lines.map((l) => [l.op, l.text, l.oldNo, l.newNo])).toEqual([
      ["same", "one", 1, 1],
      ["add", "2", null, 2],
      ["del", "two", 2, null],
      ["same", "three", 3, 3],
    ]);
    expect(diffStats(lines)).toEqual({ added: 1, removed: 1 });
  });

  it("aligns insertions in the middle and shows a new file as all added", () => {
    const { lines } = lineDiff("a\nb\nc\nd\n", "a\nb\nX\nY\nc\nd\n");
    expect(diffStats(lines)).toEqual({ added: 2, removed: 0 });
    expect(lines.filter((l) => l.op === "add").map((l) => l.newNo)).toEqual([3, 4]);
    expect(lines.at(-1)).toEqual({ op: "same", text: "d", oldNo: 4, newNo: 6 });
    const created = lineDiff("", "x\ny\n");
    expect(created.lines.every((l) => l.op === "add")).toBe(true);
    const removed = lineDiff("x\n", "");
    expect(removed.lines).toEqual([{ op: "del", text: "x", oldNo: 1, newNo: null }]);
  });

  it("falls back to a block replace when the middle is too large to align", () => {
    const n = Math.ceil(Math.sqrt(MAX_COMPARE_CELLS)) + 10;
    const a = Array.from({ length: n }, (_, i) => `a${i}`).join("\n");
    const b = Array.from({ length: n }, (_, i) => `b${i}`).join("\n");
    const { lines, exact } = lineDiff(a, b);
    expect(exact).toBe(false);
    expect(diffStats(lines)).toEqual({ added: n, removed: n });
  });

  it("keeps three lines of context around each change and drops the rest", () => {
    const before = Array.from({ length: 30 }, (_, i) => `l${i}`).join("\n");
    const after = before.replace("l2\n", "L2\n").replace("l25\n", "L25\n");
    const groups = hunks(lineDiff(before, after).lines);
    expect(groups).toHaveLength(2);
    expect(groups[0][0].text).toBe("l0");
    expect(groups[1].some((l) => l.text === "L25")).toBe(true);
    expect(groups.flat().some((l) => l.text === "l14")).toBe(false);
    expect(hunks(lineDiff("same\n", "same\n").lines)).toEqual([]);
  });

  it("describes sides that are not text in plain words", () => {
    expect(describeSide({ kind: "absent" }, "before")).toMatch(/did not exist/);
    expect(describeSide({ kind: "absent" }, "after")).toMatch(/removed/);
    expect(describeSide({ kind: "binary", size: 2048 }, "after")).toBe("Binary content (2 KB), not shown.");
    expect(describeSide({ kind: "too_large", size: 300 }, "before")).toBe("Too large to show here (300 bytes).");
    expect(describeSide({ kind: "unavailable", reason: "this step was undone" }, "after")).toBe("Not shown: this step was undone.");
    for (const k of [describeSide({ kind: "symlink", target: "x" }, "after")]) expect(k).not.toContain("—");
  });
});
