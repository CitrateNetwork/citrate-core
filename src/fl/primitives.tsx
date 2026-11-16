// =====================================================================
// citrate-core: shared FL primitives (0.5.4 FL design, board 12 "Components")
//
// The small parts every FL board, and the group, cluster and Hermes surfaces, draw with: chips,
// the HIC badge, cards, key-value lists, the "—" unknown value with its reason, notes and the
// source tag. Styles live in src/styles/fl.css against the register tokens only.
//
// Two rules these parts carry for their callers:
//  - Unknown is not zero. A failed read renders <Unknown reason="…"/>: the "—" glyph plus the
//    reason, never 0 and never an empty cell.
//  - Colour is never alone. A chip always carries its word; tone only reinforces it.
// =====================================================================
import type { CSSProperties, ReactNode } from "react";

export type Tone = "neutral" | "ok" | "warn" | "danger" | "info";

const TONE_CLASS: Record<Tone, string> = {
  neutral: "fl-chip",
  ok: "fl-chip fl-chip-ok",
  warn: "fl-chip fl-chip-warn",
  danger: "fl-chip fl-chip-danger",
  info: "fl-chip fl-chip-info",
};

/** A state word in a pill. */
export function Chip({ tone = "neutral", children, title, style }: { tone?: Tone; children: ReactNode; title?: string; style?: CSSProperties }) {
  return (
    <span className={TONE_CLASS[tone]} data-tone={tone} title={title} style={style}>
      {children}
    </span>
  );
}

export type HicLevel = "HIC-1" | "HIC-2";

const HIC_MEANING: Record<HicLevel, string> = {
  "HIC-1": "you approve this action yourself",
  "HIC-2": "inside an envelope you signed",
};

/** The authority an action runs under. Never "HITL". */
export function HicBadge({ level, style }: { level: HicLevel; style?: CSSProperties }) {
  return (
    <span className="fl-hic" data-hic={level} title={`${level}: ${HIC_MEANING[level]}`} style={style}>
      {level}
      <span className="sr-only">, {HIC_MEANING[level]}</span>
    </span>
  );
}

/** A card. `raised` uses the raised surface (sheets, confirm cards). */
export function Card({ children, raised, style, label, labelledBy, as = "div", testid }: { children: ReactNode; raised?: boolean; style?: CSSProperties; label?: string; labelledBy?: string; as?: "div" | "section" | "article"; testid?: string }) {
  const Tag = as;
  return (
    <Tag className={raised ? "fl-card-2" : "fl-card"} aria-label={label} aria-labelledby={labelledBy} data-testid={testid} style={{ padding: "12px 14px", display: "flex", flexDirection: "column", gap: 6, minWidth: 0, ...style }}>
      {children}
    </Tag>
  );
}

export interface KVRow {
  k: string;
  v: ReactNode;
  mono?: boolean;
}

/** A definition list of facts. */
export function KV({ rows, style }: { rows: KVRow[]; style?: CSSProperties }) {
  return (
    <dl className="fl-kv" style={style}>
      {rows.map((r) => (
        <div key={r.k} style={{ display: "contents" }}>
          <dt>{r.k}</dt>
          <dd className={r.mono ? "mono" : undefined}>{r.v}</dd>
        </div>
      ))}
    </dl>
  );
}

/** The unknown value: "—" and why. Never rendered as 0. */
export const UNKNOWN_GLYPH = "—";

export function Unknown({ reason, inline = true }: { reason: string; inline?: boolean }) {
  return (
    <span data-unknown="true" title={reason} style={{ display: inline ? "inline" : "block" }}>
      <span aria-hidden="true">{UNKNOWN_GLYPH}</span>
      <span className="fl-faint" style={{ fontSize: "0.92em", marginLeft: 6 }}>
        <span className="sr-only">unknown: </span>
        {reason}
      </span>
    </span>
  );
}

/** A quiet note that names the real stage ("No training starts from the app yet"). */
export function Note({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  return (
    <p className="fl-note" style={style}>
      {children}
    </p>
  );
}

/** Where a value is read from. Shown in Advanced rows; elsewhere pass it as a `title`. */
export function SourceTag({ children }: { children: string }) {
  return (
    <span className="fl-src" title={"Read from " + children}>
      {children}
    </span>
  );
}

/** A small uppercase label over a value. */
export function Eyebrow({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  return (
    <span className="eyebrow" style={{ fontSize: 10, ...style }}>
      {children}
    </span>
  );
}

/** Shorten a 0x address or hash for display: 0x25fc…53c5. The full value goes in `title`. */
export function shortHex(h: string, head = 6, tail = 4): string {
  if (h.length <= head + tail + 1) return h;
  return `${h.slice(0, head)}…${h.slice(-tail)}`;
}

/** A shortened address or hash, full value on hover and for assistive tech. */
export function Hex({ value, head, tail }: { value: string; head?: number; tail?: number }) {
  return (
    <span className="mono fl-faint" title={value}>
      {shortHex(value, head, tail)}
    </span>
  );
}
