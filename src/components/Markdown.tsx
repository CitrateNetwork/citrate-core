// =====================================================================
// Markdown — a small, dependency-free, CSP-safe markdown renderer.
//
// The agent (Hermes / gateway / local model) replies in markdown. This renders a practical GFM
// subset to REACT ELEMENTS — never dangerouslySetInnerHTML — so there is no HTML-injection surface
// (Rule 1: honest, safe surfaces).
//
// Supported: ATX headings, fenced code (``` or ~~~, any info string, indented up to 3 spaces,
// unclosed = code to the end so a mid-stream fence renders as code), blockquotes, GFM tables,
// nested unordered/ordered lists (one list across blank lines; `start` honoured), thematic breaks,
// paragraphs; inline `code`, [links](http/https/mailto), ***bold italic***, **bold**/__bold__,
// *italic*/_italic_ (never inside snake_case words), ~~strike~~.
//
// HUP-S0.4 (2026-09-30): rewritten after the owner's "the text is bad in the agent" report. The
// previous version italicized snake_case tool names, flattened nested lists, restarted numbered
// lists after blank lines, had no tables, and LOOPED FOREVER (OOM) on a fence whose language tag had
// a non-word character (```c++, ```shell-session). Every block branch below consumes ≥ 1 line.
// =====================================================================
import type { ReactNode } from "react";
import { memo } from "react";

// ---- inline ---------------------------------------------------------------

/** Only allow safe link schemes — never javascript:/data: (which a model could emit). */
function safeHref(url: string): string | null {
  const u = url.trim();
  return /^(https?:\/\/|mailto:)/i.test(u) ? u : null;
}

type Maker = (m: RegExpExecArray, k: string) => ReactNode;

// Order matters only for ties at the same index: code is opaque, then links, then the emphasis
// family from widest marker to narrowest. Emphasis requires non-space just inside the markers and
// (for `_`) no word character just outside, so `node_status`, `2 * 3 * 4`, and `Q4_0.gguf` stay
// literal.
const PATTERNS: Array<{ re: RegExp; make: Maker }> = [
  {
    re: /`([^`\n]+)`/,
    make: (m, k) => (
      <code key={k} className="mono" style={{ background: "var(--srf-2)", borderRadius: 4, padding: "1px 5px", fontSize: "0.92em" }}>
        {m[1]}
      </code>
    ),
  },
  {
    re: /\[([^\]\n]+)\]\(([^)\s]+)\)/,
    make: (m, k) => {
      const h = safeHref(m[2]);
      return h ? (
        <a key={k} href={h} target="_blank" rel="noreferrer" style={{ color: "var(--accent-text)" }}>
          {inline(m[1], k)}
        </a>
      ) : (
        <span key={k}>{m[0]}</span>
      );
    },
  },
  { re: /\*\*\*(?=\S)([^\n]+?)(?<=\S)\*\*\*/, make: (m, k) => <strong key={k}><em>{inline(m[1], k)}</em></strong> },
  { re: /\*\*(?=\S)([^\n]+?)(?<=\S)\*\*/, make: (m, k) => <strong key={k}>{inline(m[1], k)}</strong> },
  { re: /(?<![\p{L}\p{N}_])__(?=\S)([^\n]+?)(?<=\S)__(?![\p{L}\p{N}_])/u, make: (m, k) => <strong key={k}>{inline(m[1], k)}</strong> },
  { re: /~~(?=\S)([^~\n]+?)(?<=\S)~~/, make: (m, k) => <del key={k}>{inline(m[1], k)}</del> },
  { re: /(?<![\p{L}\p{N}*])\*(?=\S)([^*\n]+?)(?<=\S)\*(?![\p{L}\p{N}*])/u, make: (m, k) => <em key={k}>{inline(m[1], k)}</em> },
  { re: /(?<![\p{L}\p{N}_])_(?=\S)([^_\n]+?)(?<=\S)_(?![\p{L}\p{N}_])/u, make: (m, k) => <em key={k}>{inline(m[1], k)}</em> },
];

/** Parse inline markup into React nodes (a flat list with stable keys). */
function inline(text: string, keyPrefix: string): ReactNode[] {
  const out: ReactNode[] = [];
  let rest = text;
  let i = 0;
  while (rest.length) {
    let best: { idx: number; len: number; node: ReactNode } | null = null;
    for (const p of PATTERNS) {
      const m = p.re.exec(rest);
      if (m && m[0].length > 0 && (best === null || m.index < best.idx)) {
        best = { idx: m.index, len: m[0].length, node: p.make(m, `${keyPrefix}-i${i}`) };
      }
    }
    if (!best) {
      out.push(rest);
      break;
    }
    if (best.idx > 0) out.push(rest.slice(0, best.idx));
    out.push(best.node);
    rest = rest.slice(best.idx + best.len);
    i++;
  }
  return out;
}

// ---- block recognizers ----------------------------------------------------

const H_SIZE: Record<number, number> = { 1: 20, 2: 17, 3: 15, 4: 14, 5: 13, 6: 12.5 };
const RE_BLANK = /^\s*$/;
const RE_FENCE_OPEN = /^ {0,3}(`{3,}|~{3,})\s*([^`\s]*)[^`]*$/;
const RE_HEADING = /^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RE_HR = /^ {0,3}([-*_])(\s*\1){2,}\s*$/;
const RE_QUOTE = /^ {0,3}>\s?/;
const RE_ITEM = /^(\s*)([-*+]|(\d{1,9})[.)])\s+(.*)$/;
const RE_TABLE_SEP = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/;

const indentOf = (s: string) => s.replace(/\t/g, "    ").length;

function isTableStart(lines: string[], i: number): boolean {
  return i + 1 < lines.length && lines[i].includes("|") && RE_TABLE_SEP.test(lines[i + 1]) && lines[i + 1].includes("-");
}

function splitRow(line: string): string[] {
  const t = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  return t.split(/(?<!\\)\|/).map((c) => c.trim().replace(/\\\|/g, "|"));
}

/** A line that starts a new block (ends a paragraph). */
function startsBlock(lines: string[], i: number): boolean {
  const l = lines[i];
  return (
    RE_BLANK.test(l) ||
    RE_FENCE_OPEN.test(l) ||
    RE_HEADING.test(l) ||
    RE_QUOTE.test(l) ||
    RE_ITEM.test(l) ||
    RE_HR.test(l) ||
    isTableStart(lines, i)
  );
}

// ---- lists ----------------------------------------------------------------

const LIST_STYLE = { margin: "2px 0", paddingLeft: 20, display: "flex", flexDirection: "column" as const, gap: 1 };

/** Parse a (possibly nested) list starting at `start`. Returns the node and the next line index.
 *  Items at `baseIndent` belong to this list; deeper items nest under the previous item; a blank
 *  line continues the list when the next non-blank line is an item of the same kind at this level. */
function parseList(lines: string[], start: number, baseIndent: number, k: () => string): [ReactNode, number] {
  const first = RE_ITEM.exec(lines[start]);
  const ordered = !!first?.[3];
  const startNum = first?.[3] ? parseInt(first[3], 10) : 1;
  const items: Array<{ text: string[]; children: ReactNode[] }> = [];
  let i = start;
  while (i < lines.length) {
    const line = lines[i];
    if (RE_BLANK.test(line)) {
      let j = i + 1;
      while (j < lines.length && RE_BLANK.test(lines[j])) j++;
      const nxt = j < lines.length ? RE_ITEM.exec(lines[j]) : null;
      if (nxt && indentOf(nxt[1]) >= baseIndent && (indentOf(nxt[1]) > baseIndent || !!nxt[3] === ordered)) {
        i = j;
        continue;
      }
      break;
    }
    const m = RE_ITEM.exec(line);
    if (m) {
      const ind = indentOf(m[1]);
      if (ind < baseIndent) break;
      if (ind === baseIndent || items.length === 0) {
        if (ind === baseIndent && !!m[3] !== ordered && items.length > 0) break; // list kind changed
        items.push({ text: [m[4]], children: [] });
        i++;
        continue;
      }
      // deeper → nested list under the previous item
      const [node, next] = parseList(lines, i, ind, k);
      items[items.length - 1].children.push(node);
      i = next;
      continue;
    }
    // continuation text (indented, or lazy) belongs to the last item
    if (items.length > 0 && !startsBlock(lines, i)) {
      items[items.length - 1].text.push(line.trim());
      i++;
      continue;
    }
    break;
  }
  if (i === start) i = start + 1; // progress guarantee
  const lis = items.map((it) => {
    const key = k();
    return (
      <li key={key} style={{ margin: "1px 0" }}>
        {inline(it.text.join(" "), key)}
        {it.children}
      </li>
    );
  });
  const node = ordered ? (
    <ol key={k()} style={LIST_STYLE} start={startNum !== 1 ? startNum : undefined}>
      {lis}
    </ol>
  ) : (
    <ul key={k()} style={LIST_STYLE}>
      {lis}
    </ul>
  );
  return [node, i];
}

// ---- document -------------------------------------------------------------

function render(text: string): ReactNode {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let i = 0;
  let key = 0;
  const k = () => `md-${key++}`;

  while (i < lines.length) {
    const line = lines[i];

    if (RE_BLANK.test(line)) {
      i++;
      continue;
    }

    // fenced code block (unclosed → code to the end, so a streaming fence renders as code)
    const fence = RE_FENCE_OPEN.exec(line);
    if (fence) {
      const marker = fence[1];
      const close = new RegExp(`^ {0,3}${marker[0] === "`" ? "`" : "~"}{${marker.length},}\\s*$`);
      const buf: string[] = [];
      i++;
      while (i < lines.length && !close.test(lines[i])) {
        buf.push(lines[i]);
        i++;
      }
      i++; // closing fence (or past the end)
      blocks.push(
        <pre key={k()} className="mono" style={{ background: "var(--srf-2)", border: "1px solid var(--line-1)", borderRadius: 8, padding: "10px 12px", overflowX: "auto", fontSize: 12.5, lineHeight: 1.5, margin: "2px 0" }}>
          <code>{buf.join("\n")}</code>
        </pre>,
      );
      continue;
    }

    const h = RE_HEADING.exec(line);
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

    if (RE_HR.test(line) && !RE_ITEM.test(line)) {
      blocks.push(<hr key={k()} style={{ border: "none", borderTop: "1px solid var(--line-1)", margin: "6px 0" }} />);
      i++;
      continue;
    }

    if (isTableStart(lines, i)) {
      const head = splitRow(lines[i]);
      const aligns = splitRow(lines[i + 1]).map((c) =>
        c.startsWith(":") && c.endsWith(":") ? "center" : c.endsWith(":") ? "right" : "left",
      ) as Array<"left" | "center" | "right">;
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && !RE_BLANK.test(lines[i]) && lines[i].includes("|")) {
        rows.push(splitRow(lines[i]));
        i++;
      }
      const cell = { padding: "4px 10px", borderBottom: "1px solid var(--line-1)", verticalAlign: "top" as const };
      blocks.push(
        <div key={k()} style={{ overflowX: "auto", margin: "2px 0" }}>
          <table style={{ borderCollapse: "collapse", fontSize: "0.95em" }}>
            <thead>
              <tr>
                {head.map((c, ci) => (
                  <th key={ci} style={{ ...cell, textAlign: aligns[ci] ?? "left", fontWeight: 600 }}>
                    {inline(c, `th${ci}`)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r, ri) => (
                <tr key={ri}>
                  {head.map((_, ci) => (
                    <td key={ci} style={{ ...cell, textAlign: aligns[ci] ?? "left" }}>
                      {inline(r[ci] ?? "", `td${ri}-${ci}`)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      continue;
    }

    if (RE_QUOTE.test(line)) {
      const buf: string[] = [];
      while (i < lines.length && RE_QUOTE.test(lines[i])) {
        buf.push(lines[i].replace(RE_QUOTE, ""));
        i++;
      }
      blocks.push(
        <blockquote key={k()} style={{ borderLeft: "3px solid var(--line-2)", paddingLeft: 10, margin: "2px 0", color: "var(--tx-2)" }}>
          {inline(buf.join(" "), k())}
        </blockquote>,
      );
      continue;
    }

    const item = RE_ITEM.exec(line);
    if (item) {
      const [node, next] = parseList(lines, i, indentOf(item[1]), k);
      blocks.push(node);
      i = next;
      continue;
    }

    // paragraph — this line plus following lines that don't start a block (always ≥ 1 line)
    const buf: string[] = [line];
    i++;
    while (i < lines.length && !startsBlock(lines, i)) {
      buf.push(lines[i]);
      i++;
    }
    blocks.push(
      <p key={k()} style={{ margin: "2px 0", lineHeight: 1.6, whiteSpace: "pre-wrap" }}>
        {inline(buf.join("\n"), k())}
      </p>,
    );
  }

  return <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>{blocks}</div>;
}

/** Render a markdown string as React elements. Memoized on the text, so finished messages are not
 *  re-parsed while another message streams. */
export const Markdown = memo(function Markdown({ text }: { text: string }): ReactNode {
  return render(text);
});
