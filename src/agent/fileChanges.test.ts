// HUP-S2.9 — the sidecar's file tool results become file-change records the app can undo, and
// every undo outcome becomes an honest sentence (a refusal is never shown as success).
import { describe, it, expect } from "vitest";
import { describeUndo, isFileTool, parseFileChange, type UndoOutcome } from "./fileChanges";

const body = (o: Record<string, unknown>) => JSON.stringify(o);

describe("HUP-S2.9 file changes", () => {
  it("knows the four sidecar file tools and nothing else", () => {
    for (const t of ["fs_write", "fs_edit", "fs_delete", "fs_rename"]) expect(isFileTool(t)).toBe(true);
    for (const t of ["fs_read", "node_status", "", "FS_WRITE"]) expect(isFileTool(t)).toBe(false);
  });

  it("reads the checkpoint a file tool result names", () => {
    const c = parseFileChange("fs_rename", body({ ok: true, tool: "fs_rename", paths: ["/w/a.txt", "/w/b/a.txt"], checkpoint: { session: "s2-ab", seq: 7 } }));
    expect(c).toEqual({ session: "s2-ab", seq: 7, tool: "fs_rename", paths: ["/w/a.txt", "/w/b/a.txt"] });
  });

  it("ignores anything that is not a well-formed file change", () => {
    expect(parseFileChange("node_status", body({ checkpoint: { session: "s1", seq: 1 }, paths: [] }))).toBeNull();
    expect(parseFileChange("fs_write", "tool error: no grant")).toBeNull();
    expect(parseFileChange("fs_write", body({ paths: ["/a"] }))).toBeNull();
    expect(parseFileChange("fs_write", body({ paths: ["/a"], checkpoint: { session: "../x", seq: 1 } }))).toBeNull();
    expect(parseFileChange("fs_write", body({ paths: ["/a"], checkpoint: { session: "s1", seq: 0 } }))).toBeNull();
    expect(parseFileChange("fs_write", body({ paths: ["/a"], checkpoint: { session: "s1", seq: 1.5 } }))).toBeNull();
    expect(parseFileChange("fs_write", body({ paths: [1], checkpoint: { session: "s1", seq: 1 } }))).toBeNull();
    expect(parseFileChange("fs_write", body({ paths: [], checkpoint: { session: "s1", seq: 1 } }))).toBeNull();
  });

  it("says what an undo did, and why a refused one did nothing", () => {
    const ok: UndoOutcome = { ok: true, undone: [3, 2], restored: ["a.txt", "b.txt"], prunedThrough: null, kind: null, reason: null, conflicts: [] };
    expect(describeUndo(ok)).toBe("Undone: restored 2 files.");
    expect(describeUndo({ ...ok, restored: ["a.txt"] })).toBe("Undone: restored a.txt.");
    expect(describeUndo({ ...ok, undone: [], restored: [] })).toBe("Nothing left to undo.");
    expect(describeUndo({ ...ok, prunedThrough: 1 })).toContain("Older changes, up to step 1, were pruned");
    const conflict: UndoOutcome = {
      ok: false, undone: [], restored: [], prunedThrough: null, kind: "conflict",
      reason: "undo refused, nothing was changed: 1 path(s) changed since the step",
      conflicts: [{ seq: 4, path: "src/a.rs", found: "file sha256:0123456789ab" }],
    };
    const text = describeUndo(conflict);
    expect(text).toContain("Not undone");
    expect(text).toContain("src/a.rs changed after the agent's edit");
    expect(text).not.toMatch(/^Undone/);
    expect(describeUndo({ ...conflict, kind: "pruned", conflicts: [], reason: "step 4 was pruned" })).toBe("Not undone: step 4 was pruned");
  });
});
