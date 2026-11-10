// K3: shared pieces of the Node page tabs (design slice 2 markup): the error / unavailable card,
// a labelled row that names its source, a titled panel, and the bounded log viewer.
import type { CSSProperties, ReactNode } from "react";
import type { LogLine } from "../../shell/state";

/** Lines a log view keeps (SPEC-001 SC4 budget: 500 lines or 256 KiB). */
export const LOG_LINE_CAP = 500;

/** The error or unavailable state of a screen: what failed or is missing, and the call behind it. */
export function StateCard({
  kind,
  title,
  body,
  why,
  cta,
}: {
  kind: "error" | "unavailable";
  title: string;
  body: string;
  why?: string;
  cta?: { label: string; go: () => void };
}) {
  const bad = kind === "error";
  return (
    <div
      data-state={kind}
      className="surface"
      style={{ maxWidth: 640, padding: "18px 20px", display: "flex", flexDirection: "column", gap: 10, borderColor: bad ? "var(--danger)" : "var(--line-2)" }}
    >
      <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: bad ? "var(--danger)" : "var(--tx-3)" }}>
        {bad ? "Error" : "Unavailable"}
      </span>
      <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 19, lineHeight: 1.25 }}>{title}</span>
      <p style={{ fontSize: 13, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>{body}</p>
      {why && (
        <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", background: "var(--srf-inset)", borderRadius: "var(--r-1)", padding: "8px 10px", overflowWrap: "anywhere" }}>
          {why}
        </span>
      )}
      {cta && (
        <span>
          <button className="btn btn-ghost btn-sm" onClick={cta.go}>
            {cta.label}
          </button>
        </span>
      )}
    </div>
  );
}

/** A value with the read it came from underneath (Rule 7). */
export function Row({ k, v, src, vColor, testid }: { k: string; v: ReactNode; src: string; vColor?: string; testid?: string }) {
  return (
    <div style={{ display: "flex", alignItems: "baseline", gap: 14, padding: "10px 18px", borderBottom: "1px solid var(--line-1)" }}>
      <span style={{ width: 170, flexShrink: 0, fontSize: 12.5, color: "var(--tx-2)" }}>{k}</span>
      <span style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 2 }}>
        <span data-testid={testid} className="mono tabular" style={{ fontSize: 12.5, color: vColor ?? "var(--tx-1)", overflowWrap: "anywhere" }}>
          {v}
        </span>
        <span data-row-source className="mono" style={{ fontSize: 10, color: "var(--tx-3)", lineHeight: 1.5 }}>
          {src}
        </span>
      </span>
    </div>
  );
}

/** A card with a title bar and an optional right-hand caption. */
export function Panel({ title, caption, children, testid }: { title: string; caption?: string; children: ReactNode; testid?: string }) {
  return (
    <div className="surface" data-testid={testid} style={{ display: "flex", flexDirection: "column", minWidth: 0 }}>
      <div style={{ padding: "12px 18px", borderBottom: "1px solid var(--line-1)", display: "flex", alignItems: "center", gap: 10 }}>
        <span style={{ fontSize: 14, fontWeight: 500, flex: 1 }}>{title}</span>
        {caption && (
          <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>
            {caption}
          </span>
        )}
      </div>
      {children}
    </div>
  );
}

/** The last LOG_LINE_CAP lines of a log, or why there are none. */
export function BoundedLog({ lines, empty, testid, maxHeight = 300 }: { lines: LogLine[]; empty: string; testid: string; maxHeight?: number }) {
  const shown = lines.slice(-LOG_LINE_CAP);
  return (
    <div
      data-testid={testid}
      style={{ background: "var(--srf-inset)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: "10px 12px", display: "flex", flexDirection: "column", gap: 3, maxHeight, overflowY: "auto", overflowX: "hidden", minWidth: 0 }}
    >
      {shown.length === 0 && (
        <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
          {empty}
        </span>
      )}
      {shown.map((l) => (
        <span key={l.id} data-log-line className="mono tabular" style={{ fontSize: 10.5, lineHeight: 1.5, color: "var(--tx-2)", whiteSpace: "pre-wrap", wordBreak: "break-word", overflowWrap: "anywhere" }}>
          <span style={{ color: "var(--tx-3)" }}>{l.t}</span> {stripAnsi(l.line)}
        </span>
      ))}
    </div>
  );
}

/** "showing N of M kept" for a bounded log. */
export function logCaption(total: number): string {
  return "showing " + Math.min(total, LOG_LINE_CAP) + " of " + total + " kept · cap " + LOG_LINE_CAP + " lines";
}

// Strip ANSI colour/style escapes from node output. node.rs sets NO_COLOR in the spawn env; this is
// the backstop, so a stray escape never renders as an unfilled box glyph.
// eslint-disable-next-line no-control-regex
const ANSI_RE = /\x1b\[[0-9;]*m/g;
export function stripAnsi(s: string): string {
  return s.replace(ANSI_RE, "");
}

/** Plain text for a rejected bridge call (Tauri rejects a Rust Err(String) as a string). */
export function errText(e: unknown): string {
  return e instanceof Error ? e.message : typeof e === "string" && e.trim() ? e : "no answer";
}

/** The page padding every Node tab uses. */
export const PAGE: CSSProperties = { padding: "16px 26px 24px", display: "flex", flexDirection: "column", gap: 16, boxSizing: "border-box" };
