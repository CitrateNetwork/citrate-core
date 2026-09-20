// =====================================================================
// Markdown — a small, dependency-free, CSP-safe markdown renderer.
//
// The agent (Hermes / gateway / local gemma) replies in markdown — headings, lists, fenced code,
// bold/inline-code, links. Rendering it as a flat pre-wrap string (the old behaviour) made rich
// answers look broken. This renders a practical markdown subset to REACT ELEMENTS — never
// dangerouslySetInnerHTML — so there is no HTML-injection surface (Rule 1: honest, safe surfaces).
//
// Supported: ATX headings (#..######), fenced code ```lang, blockquotes >, unordered (-,*,+) and
// ordered (1.) lists, thematic breaks (---), paragraphs; inline **bold**, *italic*/_italic_,
// `code`, and [text](http/https/mailto) links. Anything else falls through as literal text.
// =====================================================================
import type { ReactNode } from "react";

// ---- inline ---------------------------------------------------------------

/** Only allow safe link schemes — never javascript:/data: (which a model could emit). */
function safeHref(url: string): string | null {
  const u = url.trim();
  return /^(https?:\/\/|mailto:)/i.test(u) ? u : null;
}

/** Parse a single line's inline markup into React nodes. Order matters: code first (opaque), then
 *  links, then bold, then italic. Returns a flat node list with stable keys. */
function inline(text: string, keyPrefix: string): ReactNode[] {
  const out: ReactNode[] = [];
  let rest = text;
  let i = 0;
  // Tokenizer: find the earliest of the recognized markers, emit the literal before it, then the token.
  const patterns: Array<{ re: RegExp; make: (m: RegExpExecArray, k: string) => ReactNode }> = [
    { re: /`([^`]+)`/, make: (m, k) => <code key={k} className="mono" style={{ background: "var(--srf-2)", borderRadius: 4, padding: "1px 5px", fontSize: "0.92em" }}>{m[1]}</code> },
    { re: /\[([^\]]+)\]\(([^)\s]+)\)/, make: (m, k) => { const h = safeHref(m[2]); return h ? <a key={k} href={h} target="_blank" rel="noreferrer" style={{ color: "var(--accent-text)" }}>{m[1]}</a> : <span key={k}>{m[0]}</span>; } },
    { re: /\*\*([^*]+)\*\*/, make: (m, k) => <strong key={k}>{m[1]}</strong> },
    { re: /(?:\*([^*]+)\*|_([^_]+)_)/, make: (m, k) => <em key={k}>{m[1] ?? m[2]}</em> },
  ];
  while (rest.length) {
    let best: { idx: number; len: number; node: ReactNode } | null = null;
    for (const p of patterns) {
      const m = p.re.exec(rest);
      if (m && (best === null || m.index < best.idx)) {
        best = { idx: m.index, len: m[0].length, node: p.make(m, `${keyPrefix}-i${i}`) };
      }
    }
    if (!best) { out.push(rest); break; }
    if (best.idx > 0) out.push(rest.slice(0, best.idx));
    out.push(best.node);
    rest = rest.slice(best.idx + best.len);
    i++;
  }
  return out;
}

// ---- block ----------------------------------------------------------------

const H_SIZE: Record<number, number> = { 1: 20, 2: 17, 3: 15, 4: 14, 5: 13, 6: 12.5 };

/** Render a markdown string as a block of React elements. */
export function Markdown({ text }: { text: string }): ReactNode {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let i = 0;
  let key = 0;
  const k = () => `md-${key++}`;

  while (i < lines.length) {
    const line = lines[i];

    // fenced code block
    const fence = /^```(\w+)?\s*$/.exec(line);
    if (fence) {
      const buf: string[] = [];
      i++;
      while (i < lines.length && !/^```\s*$/.test(lines[i])) { buf.push(lines[i]); i++; }
      i++; // closing fence
      blocks.push(
        <pre key={k()} className="mono" style={{ background: "var(--srf-2)", border: "1px solid var(--line-1)", borderRadius: 8, padding: "10px 12px", overflowX: "auto", fontSize: 12.5, lineHeight: 1.5, margin: "2px 0" }}>
          <code>{buf.join("\n")}</code>
        </pre>,
      );
      continue;
    }

    // blank line
    if (/^\s*$/.test(line)) { i++; continue; }

    // heading
    const h = /^(#{1,6})\s+(.*)$/.exec(line);
    if (h) {
      const lvl = h[1].length;
      blocks.push(
        <div key={k()} style={{ fontSize: H_SIZE[lvl], fontWeight: 600, lineHeight: 1.35, margin: "4px 0 1px" }}>
          {inline(h[2], k())}
        </div>,
      );
      i++;
      continue;
    }

    // thematic break
    if (/^\s*([-*_])\1{2,}\s*$/.test(line)) {
      blocks.push(<hr key={k()} style={{ border: "none", borderTop: "1px solid var(--line-1)", margin: "6px 0" }} />);
      i++;
      continue;
    }

    // blockquote (consecutive > lines)
    if (/^\s*>\s?/.test(line)) {
      const buf: string[] = [];
      while (i < lines.length && /^\s*>\s?/.test(lines[i])) { buf.push(lines[i].replace(/^\s*>\s?/, "")); i++; }
      blocks.push(
        <blockquote key={k()} style={{ borderLeft: "3px solid var(--line-2)", paddingLeft: 10, margin: "2px 0", color: "var(--tx-2)" }}>
          {inline(buf.join(" "), k())}
        </blockquote>,
      );
      continue;
    }

    // list (unordered or ordered) — a run of adjacent item lines
    if (/^\s*([-*+]|\d+\.)\s+/.test(line)) {
      const ordered = /^\s*\d+\.\s+/.test(line);
      const items: ReactNode[] = [];
      while (i < lines.length && /^\s*([-*+]|\d+\.)\s+/.test(lines[i])) {
        const content = lines[i].replace(/^\s*([-*+]|\d+\.)\s+/, "");
        items.push(<li key={k()} style={{ margin: "1px 0" }}>{inline(content, k())}</li>);
        i++;
      }
      const style = { margin: "2px 0", paddingLeft: 20, display: "flex", flexDirection: "column" as const, gap: 1 };
      blocks.push(ordered ? <ol key={k()} style={style}>{items}</ol> : <ul key={k()} style={style}>{items}</ul>);
      continue;
    }

    // paragraph — gather consecutive non-blank, non-block lines
    const buf: string[] = [];
    while (
      i < lines.length &&
      !/^\s*$/.test(lines[i]) &&
      !/^```/.test(lines[i]) &&
      !/^(#{1,6})\s+/.test(lines[i]) &&
      !/^\s*>\s?/.test(lines[i]) &&
      !/^\s*([-*+]|\d+\.)\s+/.test(lines[i]) &&
      !/^\s*([-*_])\1{2,}\s*$/.test(lines[i])
    ) { buf.push(lines[i]); i++; }
    blocks.push(
      <p key={k()} style={{ margin: "2px 0", lineHeight: 1.6, whiteSpace: "pre-wrap" }}>
        {inline(buf.join("\n"), k())}
      </p>,
    );
  }

  return <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>{blocks}</div>;
}
