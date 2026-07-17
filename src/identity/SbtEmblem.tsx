// citrate-core — the SBT identity emblem, rendered as a circular avatar.
//
// Deterministic geometric art seeded from the member's wallet address (see
// sbtArt.ts). Used as the member's identity avatar (Sidebar) and on the Wallet
// Identity tab. A member with no wallet address falls back to initials — this
// component is only rendered when a seed exists (Rule 1: never a fabricated mark).
import { sbtArtSpec, GRID } from "./sbtArt";

export function SbtArt({ seed, size, title }: { seed: string; size: number; title?: string }) {
  const spec = sbtArtSpec(seed);
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${GRID} ${GRID}`}
      shapeRendering="geometricPrecision"
      role="img"
      aria-label={title ?? "membership identity emblem"}
      style={{ borderRadius: 999, flexShrink: 0, display: "block" }}
    >
      {title ? <title>{title}</title> : null}
      <rect width={GRID} height={GRID} fill={spec.bg} />
      {spec.cells.map((cell, i) => (
        <rect key={i} x={cell.c} y={cell.r} width={1} height={1} rx={0.14} fill={cell.color} />
      ))}
    </svg>
  );
}
