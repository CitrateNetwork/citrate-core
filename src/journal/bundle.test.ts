// HUP-S10.4 — the plaintext journal bundle that is sealed for export: build,
// strict parse, and a non-destructive merge on import.
import { describe, it, expect } from "vitest";
import type { JournalPage } from "../shell/state";
import { BUNDLE_FORMAT, BUNDLE_VERSION, BundleError, buildBundle, mergeImported, parseBundle } from "./bundle";

const pages: JournalPage[] = [
  { id: "d-2026-10-01", title: "2026-10-01", kind: "daily", pinned: false, blocks: ["one", "  two"] },
  { id: "p-runbook", title: "Runbook", kind: "page", pinned: true, blocks: ["keep stake"] },
];

describe("buildBundle / parseBundle", () => {
  it("round-trips pages through the versioned bundle", () => {
    const json = buildBundle(pages, "2026-10-01T03:00:00.000Z");
    const obj = JSON.parse(json);
    expect(obj.format).toBe(BUNDLE_FORMAT);
    expect(obj.version).toBe(BUNDLE_VERSION);
    expect(obj.exportedAt).toBe("2026-10-01T03:00:00.000Z");
    expect(parseBundle(json)).toEqual(pages);
  });

  it("rejects non-JSON, a foreign format, and an unknown version", () => {
    expect(() => parseBundle("not json")).toThrow(BundleError);
    expect(() => parseBundle(JSON.stringify({ format: "other", version: 1, pages: [] }))).toThrow(/not a Citrate journal/);
    expect(() => parseBundle(JSON.stringify({ format: BUNDLE_FORMAT, version: 99, pages: [] }))).toThrow(/version 99/);
  });

  it("rejects malformed pages rather than importing partial garbage", () => {
    const bad = (p: unknown) => JSON.stringify({ format: BUNDLE_FORMAT, version: BUNDLE_VERSION, exportedAt: "x", pages: [p] });
    expect(() => parseBundle(bad({ id: "x", title: "t", kind: "daily", pinned: false, blocks: [1] }))).toThrow(BundleError);
    expect(() => parseBundle(bad({ id: "", title: "t", kind: "page", pinned: false, blocks: [] }))).toThrow(BundleError);
    expect(() => parseBundle(bad({ id: "x", title: "t", kind: "weird", pinned: false, blocks: [] }))).toThrow(BundleError);
    expect(() => parseBundle(bad(null))).toThrow(BundleError);
  });
});

describe("mergeImported — never overwrites what is already on this device", () => {
  it("adds pages that are new here", () => {
    const r = mergeImported([pages[0]], [pages[1]]);
    expect(r.pages.map((p) => p.id)).toEqual(["d-2026-10-01", "p-runbook"]);
    expect(r.added).toBe(1);
    expect(r.unchanged).toBe(0);
    expect(r.copied).toBe(0);
  });

  it("skips identical pages", () => {
    const r = mergeImported(pages, pages);
    expect(r.pages).toEqual(pages);
    expect(r.unchanged).toBe(2);
    expect(r.added + r.copied).toBe(0);
  });

  it("keeps a differing local page and adds the imported one as a separate named copy", () => {
    const local: JournalPage = { ...pages[0], blocks: ["local edit"] };
    const r = mergeImported([local], [pages[0]]);
    expect(r.pages[0]).toEqual(local);
    expect(r.copied).toBe(1);
    const copy = r.pages[1];
    expect(copy.kind).toBe("page");
    expect(copy.title).toBe("2026-10-01 (imported)");
    expect(copy.blocks).toEqual(pages[0].blocks);
    expect(copy.id).not.toBe(local.id);
    // one entry per day still holds
    expect(r.pages.filter((p) => p.kind === "daily" && p.title === "2026-10-01")).toHaveLength(1);
  });

  it("imports pin state as unpinned (a pin is a property of this device, not the file)", () => {
    const r = mergeImported([], [pages[1]]);
    expect(r.pages[0].pinned).toBe(false);
  });

  it("gives repeated imports of the same conflicting page unique ids", () => {
    const local: JournalPage = { ...pages[0], blocks: ["local edit"] };
    const once = mergeImported([local], [pages[0]]);
    const twice = mergeImported(once.pages, [{ ...pages[0], blocks: ["third version"] }]);
    const ids = twice.pages.map((p) => p.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});
