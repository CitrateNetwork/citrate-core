// HUP-S3.2 (US-3.2 AC2): intake rewrite for third-party SKILL.md frontmatter.
//
// The runtime loader (citrate-agent-runtime agent-loop/src/skills.rs) accepts a small, strict YAML
// subset: plain or quoted scalars, block scalars, one-level maps and lists, and a fixed key set.
// Owner decision (default, HUP-S3.2): keep it strict. A source whose skills use a richer
// frontmatter (the hermes-agent fork: nested `metadata.hermes.*`, `platforms`, `author`, lists of
// maps) is rewritten once, at intake: every value the loader cannot take at the top level moves
// into `metadata` as a string (a list of scalars joined with ", ", anything deeper as JSON), and
// the body is kept byte for byte. skills.lock pins the upstream hash and the shipped (rewritten)
// hash, and records the rewrite. Anything this module cannot represent faithfully (anchors,
// aliases, tags, flow maps, a value over 1024 characters, a key twice) is refused, so the skill
// stays excluded rather than shipping a guess.
//
// Zero dependencies.

/** The rewrite's name in skills.lock (`intake_rewrite`) and in the shipped `metadata`. */
export const INTAKE_REWRITE = "flatten-frontmatter";

/** Mirrors the loader's MAX_VALUE_LEN. */
const MAX_VALUE_LEN = 1024;
/** Top-level keys the loader takes as strings. */
const STRING_KEYS = ["name", "description", "license", "compatibility"];
/** Claude Code extension keys the loader tolerates (strings or lists). */
const EXTENSION_KEYS = ["argument-hint", "disable-model-invocation", "user-invocable", "model", "type", "version"];

class RewriteError extends Error {}
const fail = (msg) => {
  throw new RewriteError(`intake rewrite: ${msg}`);
};

const indentOf = (l) => l.length - l.replace(/^ +/, "").length;
const isBlank = (l) => l.trim() === "" || l.trim().startsWith("#");

/** One inline scalar (plain, single- or double-quoted). */
function scalar(raw) {
  const s = raw.trim();
  if (s.startsWith('"')) {
    if (!s.endsWith('"') || s.length < 2) fail(`unterminated string ${s.slice(0, 40)}`);
    const inner = s.slice(1, -1);
    let out = "";
    for (let i = 0; i < inner.length; i++) {
      const c = inner[i];
      if (c === "\\") {
        const n = inner[++i];
        const map = { '"': '"', "\\": "\\", "/": "/", n: "\n", t: "\t" };
        if (!(n in map)) fail(`unsupported escape \\${n}`);
        out += map[n];
      } else {
        out += c;
      }
    }
    return out;
  }
  if (s.startsWith("'")) {
    if (!s.endsWith("'") || s.length < 2) fail(`unterminated string ${s.slice(0, 40)}`);
    return s.slice(1, -1).replaceAll("''", "'");
  }
  const first = s[0];
  if (first === "&") fail("anchors are not supported");
  if (first === "*") fail("aliases are not supported");
  if (first === "!") fail("tags are not supported");
  if (first === "{") fail("flow maps are not supported");
  const i = s.indexOf(" #");
  return i >= 0 ? s.slice(0, i).trimEnd() : s;
}

/** A flow list `[a, "b, c", d]` of scalars. */
function flowList(raw) {
  const s = raw.trim();
  if (!s.endsWith("]")) fail("unterminated flow list");
  const inner = s.slice(1, -1);
  if (/[[{]/.test(inner)) fail("nested flow collections are not supported");
  const items = [];
  let cur = "";
  let q = null;
  for (const c of inner) {
    if (q) {
      cur += c;
      if (c === q) q = null;
    } else if (c === '"' || c === "'") {
      q = c;
      cur += c;
    } else if (c === ",") {
      items.push(cur);
      cur = "";
    } else {
      cur += c;
    }
  }
  items.push(cur);
  return items.map((p) => p.trim()).filter((p) => p.length).map(scalar);
}

/** Lines of a block scalar (`>` folded or `|` literal) below `indent`, as the loader joins them. */
function blockScalar(lines, i, indent, style) {
  const parts = [];
  let base = null;
  while (i < lines.length && (lines[i].trim() === "" || indentOf(lines[i]) > indent)) {
    const l = lines[i];
    if (l.trim() === "") parts.push("");
    else {
      if (base === null) base = indentOf(l);
      if (indentOf(l) < base) fail("block scalar indentation");
      parts.push(l.slice(base).trimEnd());
    }
    i++;
  }
  while (parts.length && parts[parts.length - 1] === "") parts.pop();
  if (style.startsWith("|")) return [parts.join("\n"), i];
  let s = "";
  for (const p of parts) {
    if (p === "") s += "\n";
    else {
      if (s && !s.endsWith("\n")) s += " ";
      s += p;
    }
  }
  return [s, i];
}

/** The value after `key:` on a line, plus any nested block below `indent`. */
function valueAt(lines, i, indent, rest) {
  const r = rest.trim();
  if ([">", "|", ">-", "|-", ">+", "|+"].includes(r)) return blockScalar(lines, i, indent, r);
  if (r.startsWith("[")) return [flowList(r), i];
  if (r !== "") {
    // A plain scalar may continue on more-indented lines (folded with spaces).
    let s = scalar(r);
    while (i < lines.length && !isBlank(lines[i]) && indentOf(lines[i]) > indent && !/^"|^'/.test(r)) {
      s += " " + lines[i].trim();
      i++;
    }
    return [s, i];
  }
  while (i < lines.length && isBlank(lines[i])) i++;
  if (i >= lines.length) return ["", i];
  const child = indentOf(lines[i]);
  const dash = lines[i].trim().startsWith("- ") || lines[i].trim() === "-";
  if (child > indent) return dash ? parseList(lines, i, child) : parseMap(lines, i, child);
  if (child === indent && dash) return parseList(lines, i, child);
  return ["", i];
}

function parseMap(lines, i, indent) {
  const map = new Map();
  while (i < lines.length) {
    if (isBlank(lines[i])) {
      i++;
      continue;
    }
    const l = lines[i];
    const ind = indentOf(l);
    if (ind < indent) break;
    if (ind > indent) fail(`unexpected indentation: ${l.trim().slice(0, 40)}`);
    const t = l.trim();
    if (t.startsWith("- ")) break;
    const at = t.indexOf(":");
    if (at <= 0) fail(`expected key: value, got ${t.slice(0, 40)}`);
    const key = t.slice(0, at).trim();
    const rest = t.slice(at + 1);
    if (rest && !rest.startsWith(" ")) fail(`expected a space after ':' in ${t.slice(0, 40)}`);
    if (map.has(key)) fail(`key '${key}' appears twice`);
    const [v, next] = valueAt(lines, i + 1, indent, rest);
    map.set(key, v);
    i = next;
  }
  return [map, i];
}

function parseList(lines, i, indent) {
  const list = [];
  while (i < lines.length) {
    if (isBlank(lines[i])) {
      i++;
      continue;
    }
    const l = lines[i];
    if (indentOf(l) !== indent || !(l.trim().startsWith("- ") || l.trim() === "-")) break;
    const item = l.trim().slice(1).trim();
    const at = item.indexOf(": ");
    if (at > 0 && !/^["'[]/.test(item)) {
      // A map item: `- key: value` with further keys two columns in.
      const inner = " ".repeat(indent + 2);
      const sub = [inner + item];
      let j = i + 1;
      while (j < lines.length && (isBlank(lines[j]) || indentOf(lines[j]) > indent)) sub.push(lines[j++]);
      const [m] = parseMap(sub, 0, indent + 2);
      list.push(m);
      i = j;
    } else if (item.startsWith("[")) {
      list.push(flowList(item));
      i++;
    } else {
      list.push(item === "" ? "" : scalar(item));
      i++;
    }
  }
  return [list, i];
}

const isScalar = (v) => typeof v === "string";

function plain(v) {
  if (isScalar(v)) return v;
  if (Array.isArray(v)) return v.map(plain);
  return Object.fromEntries([...v.entries()].map(([k, x]) => [k, plain(x)]));
}

/** A metadata string for any value. */
function render(v) {
  if (isScalar(v)) return v;
  if (Array.isArray(v) && v.every(isScalar)) return v.join(", ");
  return JSON.stringify(plain(v));
}

function quote(s) {
  return (
    '"' +
    s.replaceAll("\\", "\\\\").replaceAll('"', '\\"').replaceAll("\n", "\\n").replaceAll("\t", "\\t") +
    '"'
  );
}

/**
 * Rewrite a SKILL.md so its frontmatter is in the strict loader subset. Returns the new text
 * (frontmatter rewritten, body unchanged). Throws when it cannot be done faithfully.
 */
export function flattenFrontmatter(input) {
  let text = input.startsWith("\ufeff") ? input.slice(1) : input;
  if (text.includes("\r")) text = text.replaceAll("\r\n", "\n");
  if (!text.startsWith("---\n")) fail("no YAML frontmatter (expected a leading ---)");
  const after = text.slice(4);
  let offset = 0;
  let close = null;
  for (const line of after.split(/(?<=\n)/)) {
    if (line.replace(/\n$/, "").trimEnd() === "---") {
      close = [offset, offset + line.length];
      break;
    }
    offset += line.length;
  }
  if (!close) fail("frontmatter has no closing ---");
  const fmText = after.slice(0, close[0]);
  const body = after.slice(close[1]);
  if (fmText.includes("\t")) fail("tabs in the frontmatter are not supported");
  const lines = fmText.replace(/\n$/, "").split("\n");
  const [top] = parseMap(lines, 0, 0);

  const head = [];
  const meta = new Map();
  const put = (k, v) => {
    if (!/^[A-Za-z0-9_-]{1,64}$/.test(k)) fail(`metadata key '${k}' is not a plain key`);
    if (meta.has(k)) fail(`metadata key '${k}' appears twice after flattening`);
    if ([...v].length > MAX_VALUE_LEN) fail(`metadata '${k}' is longer than ${MAX_VALUE_LEN} characters`);
    meta.set(k, v);
  };
  for (const [k, v] of top) {
    if (STRING_KEYS.includes(k) && isScalar(v)) {
      head.push(`${k}: ${quote(v)}`);
    } else if (EXTENSION_KEYS.includes(k) && (isScalar(v) || (Array.isArray(v) && v.every(isScalar)))) {
      head.push(`${k}: ${quote(render(v))}`);
    } else if (k === "allowed-tools" && (isScalar(v) || (Array.isArray(v) && v.every(isScalar)))) {
      head.push(`${k}: ${quote(isScalar(v) ? v : v.join(" "))}`);
    } else if (k === "metadata" && v instanceof Map) {
      for (const [mk, mv] of v) {
        if (mv instanceof Map) for (const [sk, sv] of mv) put(`${mk}-${sk}`, render(sv));
        else put(mk, render(mv));
      }
    } else if (k === "metadata" && v === "") {
      // An empty metadata key carries nothing.
    } else {
      put(k, render(v));
    }
  }
  put("intake-rewrite", INTAKE_REWRITE);
  const out = ["---", ...head, "metadata:"];
  for (const [k, v] of meta) out.push(`  ${k}: ${quote(v)}`);
  out.push("---");
  return out.join("\n") + "\n" + body;
}
