// The SBT emblem must be DETERMINISTIC (same wallet → identical art, every
// session, reproducible by any party from the on-chain owner address) and
// symmetric. If this drifts, a member's identity mark would change under them.
import { describe, it, expect } from "vitest";
import { sbtArtSpec, sbtArtSvg, hashSeed, GRID } from "./sbtArt";

const ADDR_A = "0x9858effd232b4033e47d90003d41ec34ecaeda94";
const ADDR_B = "0xb58b96a056f20cb86926f6ed21940f8eef097e34";

describe("sbtArt determinism", () => {
  it("same seed → byte-identical SVG (stable across calls)", () => {
    expect(sbtArtSvg(ADDR_A, 28)).toBe(sbtArtSvg(ADDR_A, 28));
    expect(sbtArtSpec(ADDR_A)).toEqual(sbtArtSpec(ADDR_A));
  });

  it("is case-insensitive on the address (checksum vs lowercase are the same member)", () => {
    expect(sbtArtSvg(ADDR_A.toUpperCase(), 28)).toBe(sbtArtSvg(ADDR_A, 28));
  });

  it("different seeds → different art (no collision on these addresses)", () => {
    expect(sbtArtSvg(ADDR_A, 28)).not.toBe(sbtArtSvg(ADDR_B, 28));
    expect(hashSeed(ADDR_A)).not.toBe(hashSeed(ADDR_B));
  });

  it("is vertically symmetric (mirrored columns share fills)", () => {
    const { cells } = sbtArtSpec(ADDR_A);
    for (const cell of cells) {
      const mirror = cells.find((o) => o.c === GRID - 1 - cell.c && o.r === cell.r);
      expect(mirror, `mirror of (${cell.c},${cell.r})`).toBeTruthy();
      expect(mirror?.color).toBe(cell.color);
    }
  });

  it("an empty seed yields no cells (caller falls back to initials — never a fabricated mark)", () => {
    expect(sbtArtSpec("").cells).toEqual([]);
    expect(sbtArtSpec("  ").cells).toEqual([]);
  });

  it("produces a non-empty, bounded board for a real address", () => {
    const { cells } = sbtArtSpec(ADDR_A);
    expect(cells.length).toBeGreaterThan(0);
    expect(cells.length).toBeLessThanOrEqual(GRID * GRID);
    for (const c of cells) {
      expect(c.c).toBeGreaterThanOrEqual(0);
      expect(c.c).toBeLessThan(GRID);
      expect(c.r).toBeGreaterThanOrEqual(0);
      expect(c.r).toBeLessThan(GRID);
    }
  });
});
