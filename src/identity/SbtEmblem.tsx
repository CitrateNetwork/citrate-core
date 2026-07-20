// citrate-core — the SBT identity emblem, rendered as a circular avatar.
//
// TWO honest sources, precedence documented (Rule 1 — the source is always named):
//   1. The AUTHORITATIVE on-chain emblem. The post-reroll CitrateMemberSBT renders
//      the member emblem WHOLLY ON-CHAIN; `bridge.membership.sbtArt(sub)` resolves
//      the tokenId from keccak256(sub) and returns the tokenURI's
//      `data:image/svg+xml;base64,...` image. This is the CANONICAL mark — what an
//      explorer would render — so it WINS when available (`OnChainSbtEmblem`).
//   2. The LOCAL deterministic emblem (`sbtArt.ts`), an honest OFFLINE FALLBACK
//      seeded from the wallet address, shown ONLY when the on-chain read is
//      unavailable (web preview / no SBT / read error). It is a "local preview",
//      never presented as the on-chain art.
//
// A member with no wallet-address seed AND no on-chain art falls back to initials
// upstream; this component is rendered only when at least a seed exists.
import { useEffect, useState } from "react";
import { sbtArtSpec, GRID } from "./sbtArt";
import { bridge } from "../bridge";

/** The local, deterministic offline-fallback emblem (seeded from the wallet
 * address). This is a LOCAL PREVIEW — never the authoritative on-chain art. */
export function SbtArt({ seed, size, title }: { seed: string; size: number; title?: string }) {
  const spec = sbtArtSpec(seed);
  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${GRID} ${GRID}`}
      shapeRendering="geometricPrecision"
      role="img"
      aria-label={title ?? "membership identity emblem (local preview)"}
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

/**
 * The identity emblem with the ON-CHAIN art authoritative: it reads the member's
 * wholly-on-chain SBT emblem via `bridge.membership.sbtArt(sub)` and renders that
 * `data:image/svg+xml;base64,...` when present; otherwise it falls back to the
 * LOCAL deterministic `SbtArt` seeded from `seed` (the wallet address). The two
 * cases are visually identical avatars but semantically distinct — callers that
 * show a caption should read `onSource` (via `onResolved`) to name the source
 * honestly ("on-chain emblem" vs "local preview"). Never fabricates an on-chain
 * mark: a null read renders the labelled local fallback.
 */
export function OnChainSbtEmblem({
  sub,
  seed,
  size,
  title,
  onResolved,
}: {
  sub: string | null;
  seed: string;
  size: number;
  title?: string;
  /** Called with the resolved source once the read settles, for honest captions. */
  onResolved?: (source: "onchain" | "local") => void;
}) {
  const [image, setImage] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    // No sub → no way to resolve the on-chain token; render the local fallback.
    if (!sub) {
      setImage(null);
      onResolved?.("local");
      return;
    }
    // async/await + try/catch so a failed read is fully consumed here (never a
    // floating rejected promise): the catch renders the local fallback honestly.
    void (async () => {
      try {
        const uri = await bridge.membership.sbtArt(sub);
        if (!live) return;
        setImage(uri);
        onResolved?.(uri ? "onchain" : "local");
      } catch {
        // Honest fallback: the read failed (unwired/offline) → local preview, never
        // a fabricated on-chain mark.
        if (!live) return;
        setImage(null);
        onResolved?.("local");
      }
    })();
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sub]);

  if (image) {
    // The AUTHORITATIVE on-chain SVG data-URI, rendered directly.
    return (
      <img
        src={image}
        width={size}
        height={size}
        alt={title ?? "membership identity emblem (on-chain)"}
        style={{ borderRadius: 999, flexShrink: 0, display: "block" }}
      />
    );
  }
  // Offline fallback: the local deterministic preview.
  return <SbtArt seed={seed} size={size} title={title ?? "membership identity emblem (local preview)"} />;
}
