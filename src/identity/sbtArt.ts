// citrate-core — deterministic SBT identity art.
//
// The membership SBT (CitrateMemberSBT) is soulbound and 1:1 with the member's
// wallet address. We render a DETERMINISTIC geometric emblem seeded from that
// address, so every member has a stable, unique identity mark that any party can
// reproduce from purely on-chain data (the owner address) — no network, no IPFS,
// no fabricated token image (Rule 1).
//
// This is the CANONICAL art algorithm. The on-chain `tokenURI` pipeline (a
// contract that exposes `setTokenURI` + a pinning endpoint) does not exist yet
// (CitrateMemberSBT has no tokenURI/setTokenURI; no IPFS pinning service —
// WO-4/WO-5, [chain]/[cm]/[dgx]). When it lands it must mirror THIS algorithm +
// seed so the in-app icon matches what an explorer renders.
//
// Pure + deterministic (no Date/Math.random) so the emblem is stable across
// sessions and unit-testable.

/** A curated set of on-brand palettes (dark backdrop + two foreground hues).
 * Drawn from the brand tokens (citrate green family) plus tasteful secondary
 * accents; no alarm red. The seed selects one palette + arranges the geometry. */
const PALETTES: { bg: string; a: string; b: string }[] = [
  { bg: "#0e1a13", a: "#8ecc09", b: "#4f8a05" }, // citrate green
  { bg: "#0e1a13", a: "#5a8205", b: "#b9c6bd" }, // deep green + zone silver
  { bg: "#101c22", a: "#8ecc09", b: "#3f8fb0" }, // green + teal
  { bg: "#0f1622", a: "#6fb0e0", b: "#1b4965" }, // info blue
  { bg: "#1a140e", a: "#e0a83f", b: "#b07b00" }, // amber
  { bg: "#141021", a: "#a98fe0", b: "#6f5ab0" }, // violet
  { bg: "#0e1a13", a: "#8ecc09", b: "#e0a83f" }, // green + amber
  { bg: "#111", a: "#b9c6bd", b: "#5a8205" }, // graphite + green
];

/** The grid is GRID×GRID cells; only the left `HALF` columns are generated and
 * mirrored to the right for vertical symmetry (an identicon-style emblem). */
export const GRID = 5;
const HALF = Math.ceil(GRID / 2); // 3 -> columns 0,1,2 mirror to 4,3

/** FNV-1a 32-bit hash of the (lowercased) seed — deterministic, dependency-free. */
export function hashSeed(seed: string): number {
  let h = 0x811c9dc5;
  const s = (seed || "").toLowerCase();
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    // 32-bit FNV prime multiply (kept in uint32 via >>> 0)
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

/** mulberry32 PRNG — a small deterministic generator seeded by a uint32. */
function mulberry32(a: number): () => number {
  return function () {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export interface SbtArtCell {
  c: number;
  r: number;
  color: string;
}

export interface SbtArtSpec {
  bg: string;
  /** the two foreground hues used (palette a,b) — for callers that want a ring. */
  palette: [string, string];
  /** filled cells on the GRID×GRID board (already mirrored). */
  cells: SbtArtCell[];
}

/**
 * Build the deterministic art spec for a seed (the member's wallet address).
 * Same seed → identical spec, always. A blank/absent seed yields an empty board
 * (the caller then falls back to initials — never a fabricated emblem).
 */
export function sbtArtSpec(seed: string): SbtArtSpec {
  const clean = (seed || "").trim();
  const rng = mulberry32(hashSeed(clean));
  const pal = PALETTES[Math.floor(rng() * PALETTES.length) % PALETTES.length];
  const cells: SbtArtCell[] = [];
  if (!clean) return { bg: pal.bg, palette: [pal.a, pal.b], cells };

  for (let c = 0; c < HALF; c++) {
    for (let r = 0; r < GRID; r++) {
      // ~55% fill; the centre column (c === HALF-1 when GRID is odd) is not
      // mirrored, giving a stable vertical axis.
      if (rng() < 0.55) {
        const color = rng() < 0.62 ? pal.a : pal.b;
        cells.push({ c, r, color });
        const mc = GRID - 1 - c;
        if (mc !== c) cells.push({ c: mc, r, color });
      }
    }
  }
  return { bg: pal.bg, palette: [pal.a, pal.b], cells };
}

/**
 * Deterministic SVG string for the emblem (used by the React component and by
 * determinism tests). `size` is the pixel square; the art is clipped to a circle.
 */
export function sbtArtSvg(seed: string, size: number): string {
  const spec = sbtArtSpec(seed);
  const rects = spec.cells
    .map((cell) => `<rect x="${cell.c}" y="${cell.r}" width="1" height="1" rx="0.14" fill="${cell.color}"/>`)
    .join("");
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${GRID} ${GRID}" ` +
    `shape-rendering="geometricPrecision" role="img">` +
    `<rect width="${GRID}" height="${GRID}" fill="${spec.bg}"/>${rects}</svg>`
  );
}
