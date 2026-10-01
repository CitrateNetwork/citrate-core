// @vitest-environment node
// HUP-S7.7 — the paraconsensus + precompile literacy skills under src-tauri/skills/.
//
// Each skill is an agentskills.io SKILL.md that the runtime loader (citrate-agent-runtime
// agent-loop/src/skills.rs, HUP-S3.2) must accept. This test mirrors that loader's rules so a skill
// that the sidecar would refuse fails here first:
//   - the file starts with a `---` fence and has a closing `---` line, and is at most 64 KiB;
//   - the frontmatter uses only the accepted keys (no unknown or duplicate keys), top-level keys are
//     unindented, `metadata` is a one-level map of scalars, no tabs, anchors, aliases or tags;
//   - `name` is 1-64 chars of [a-z0-9-] with no leading, trailing or doubled hyphen, and equals the
//     skill's directory name; `description` is 1-1024 chars.
// It also checks Agentile Rule 5 (created/branch/author/status in metadata) and that every source
// citation in a body (`<repo>:<path>` or `<repo>:<path>#<symbol>` in backticks) names a repo pinned in
// that skill's metadata by full commit. When the source repos are checked out (QA_SOURCES_ROOT or
// sibling dirs), every cited path is re-read at its pinned commit and every #symbol must occur in it.
import { describe, it, expect } from "vitest";
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const skillsRoot = join(repoRoot, "src-tauri", "skills");
const sourcesRoot = process.env.QA_SOURCES_ROOT ?? resolve(repoRoot, "..");

const LITERACY_SKILLS = [
  "citrate-paraconsensus",
  "citrate-belnap-aggregate",
  "citrate-precompiles",
  "citrate-sidecar-consensus",
];

const SPEC_KEYS = ["name", "description", "license", "compatibility", "metadata", "allowed-tools"];
const EXTENSION_KEYS = ["argument-hint", "disable-model-invocation", "user-invocable", "model", "type", "version"];
const MAX_SKILL_FILE_BYTES = 64 * 1024;

interface ParsedSkill {
  fields: Map<string, string>;
  metadata: Map<string, string>;
  body: string;
}

const validKey = (k: string) => /^[A-Za-z0-9_-]{1,64}$/.test(k);
export const validSkillName = (n: string) =>
  n.length >= 1 && n.length <= 64 && /^[a-z0-9-]+$/.test(n) && !n.startsWith("-") && !n.endsWith("-") && !n.includes("--");

function scalar(raw: string, where: string): string {
  const s = raw.trim();
  if (s.startsWith('"')) {
    if (!s.endsWith('"') || s.length < 2) throw new Error(`${where}: unterminated double-quoted string`);
    return s.slice(1, -1);
  }
  if (s.startsWith("'")) {
    if (!s.endsWith("'") || s.length < 2) throw new Error(`${where}: unterminated single-quoted string`);
    return s.slice(1, -1).replace(/''/g, "'");
  }
  if (/^[&*!{@`%]/.test(s)) throw new Error(`${where}: YAML feature outside the accepted subset`);
  const i = s.indexOf(" #");
  return (i >= 0 ? s.slice(0, i) : s).trimEnd();
}

/** The accepted subset of the runtime loader: flat keys, one-level `metadata` map, no block scalars needed here. */
export function parseSkillMd(text: string): ParsedSkill {
  if (Buffer.byteLength(text, "utf8") > MAX_SKILL_FILE_BYTES) throw new Error("SKILL.md larger than 64 KiB");
  if (!text.startsWith("---\n")) throw new Error("no YAML frontmatter (expected a leading ---)");
  const rest = text.slice(4);
  const lines = rest.split("\n");
  const close = lines.findIndex((l) => l.trimEnd() === "---");
  if (close < 0) throw new Error("frontmatter has no closing ---");
  const fm = lines.slice(0, close);
  const body = lines.slice(close + 1).join("\n");
  const fields = new Map<string, string>();
  const metadata = new Map<string, string>();
  for (let i = 0; i < fm.length; i++) {
    const l = fm[i];
    const where = `frontmatter line ${i + 1}`;
    if (l.includes("\t")) throw new Error(`${where}: tabs are not supported`);
    if (l.trim() === "" || l.trim().startsWith("#")) continue;
    if (/^\s/.test(l)) throw new Error(`${where}: unexpected indentation`);
    const colon = l.indexOf(":");
    if (colon < 0) throw new Error(`${where}: expected key: value`);
    const key = l.slice(0, colon).trim();
    const value = l.slice(colon + 1);
    if (!validKey(key)) throw new Error(`${where}: invalid key ${key}`);
    if (value !== "" && !value.startsWith(" ")) throw new Error(`${where}: expected a space after ':'`);
    if (!SPEC_KEYS.includes(key) && !EXTENSION_KEYS.includes(key)) throw new Error(`unknown frontmatter key '${key}'`);
    if (fields.has(key)) throw new Error(`frontmatter key '${key}' appears twice`);
    if (key === "metadata") {
      if (value.trim() !== "") throw new Error(`${where}: 'metadata' must be a map of strings`);
      let indent = -1;
      while (i + 1 < fm.length && /^\s+\S/.test(fm[i + 1])) {
        i++;
        const m = fm[i];
        const ind = m.length - m.trimStart().length;
        if (indent < 0) indent = ind;
        if (ind !== indent) throw new Error(`frontmatter line ${i + 1}: nested collections are not supported`);
        const c = m.indexOf(":");
        if (c < 0) throw new Error(`frontmatter line ${i + 1}: expected key: value`);
        const mk = m.slice(0, c).trim();
        const mv = m.slice(c + 1);
        if (!validKey(mk) || mv.trim() === "") throw new Error(`frontmatter line ${i + 1}: bad metadata entry`);
        if (metadata.has(mk)) throw new Error(`frontmatter key 'metadata.${mk}' appears twice`);
        metadata.set(mk, scalar(mv, `frontmatter line ${i + 1}`));
      }
      fields.set(key, "");
      continue;
    }
    if (/^\s*[>|]/.test(value)) throw new Error(`${where}: block scalars are not used by these skills`);
    fields.set(key, scalar(value, where));
  }
  return { fields, metadata, body };
}

interface Citation {
  source: string;
  path: string;
  symbol?: string;
}

const CITE_RE = /`((?:citrate|agentile)-[a-z-]+):([A-Za-z0-9_./-]+\.[A-Za-z0-9]+)(?:#([^`]+))?`/g;

export function extractSkillCitations(body: string): Citation[] {
  const out: Citation[] = [];
  for (const m of body.matchAll(CITE_RE)) out.push({ source: m[1], path: m[2], symbol: m[3] });
  return out;
}

function git(dir: string, args: string[]): string | null {
  try {
    return execFileSync("git", ["-C", dir, ...args], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], maxBuffer: 1 << 26 });
  } catch {
    return null;
  }
}

describe("SKILL.md subset parser (mirror of the runtime loader rules)", () => {
  const good = "---\nname: a-skill\ndescription: Does a thing.\nmetadata:\n  created: 2026-10-01\n  status: active\n---\n# Body\n";
  it("accepts a well-formed skill and returns its body", () => {
    const p = parseSkillMd(good);
    expect(p.fields.get("name")).toBe("a-skill");
    expect(p.metadata.get("status")).toBe("active");
    expect(p.body).toContain("# Body");
  });
  it("refuses what the loader refuses", () => {
    const bad = [
      "name: x\n---\n",
      "---\nname: x\ndescription: y\n",
      "---\nname: x\ndescription: y\ncreated: 2026-10-01\n---\n",
      "---\nname: x\nname: x\ndescription: y\n---\n",
      "---\nname: x\ndescription: y\nmetadata:\n  a: 1\n    b: 2\n---\n",
      "---\nname: x\ndescription: &a y\n---\n",
      "---\n\tname: x\n---\n",
      "---\nname: x\ndescription: y\nmetadata:\n  a:\n---\n",
    ];
    for (const b of bad) expect(() => parseSkillMd(b), JSON.stringify(b)).toThrow();
  });
  it("validates skill names the agentskills.io way", () => {
    for (const n of ["citrate-precompiles", "a", "x1-y2"]) expect(validSkillName(n), n).toBe(true);
    for (const n of ["", "-a", "a-", "a--b", "Citrate", "a_b", "x".repeat(65)]) expect(validSkillName(n), n).toBe(false);
  });
  it("extracts backticked repo:path[#symbol] citations only", () => {
    const c = extractSkillCitations(
      "See `citrate-chain:core/execution/src/precompiles/mod.rs#execute_pure_at`, `citrate-chain:a/b.rs#pub fn x` and " +
        "`citrate-docs:content/chain/precompiles.md`, not citrate-chain:x.rs.",
    );
    expect(c).toEqual([
      { source: "citrate-chain", path: "core/execution/src/precompiles/mod.rs", symbol: "execute_pure_at" },
      { source: "citrate-chain", path: "a/b.rs", symbol: "pub fn x" },
      { source: "citrate-docs", path: "content/chain/precompiles.md", symbol: undefined },
    ]);
  });
});

describe("the literacy skills under src-tauri/skills", () => {
  it("are all present, one directory per skill", () => {
    const dirs = readdirSync(skillsRoot).filter((d) => statSync(join(skillsRoot, d)).isDirectory());
    for (const s of LITERACY_SKILLS) expect(dirs).toContain(s);
  });

  for (const name of LITERACY_SKILLS) {
    describe(name, () => {
      const text = readFileSync(join(skillsRoot, name, "SKILL.md"), "utf8");
      const skill = parseSkillMd(text);

      it("parses under the loader rules, and its name matches its directory", () => {
        expect(skill.fields.get("name")).toBe(name);
        const d = skill.fields.get("description") ?? "";
        expect(d.length).toBeGreaterThan(0);
        expect([...d].length).toBeLessThanOrEqual(1024);
      });
      it("carries Rule-5 frontmatter (created, branch, author, status) in metadata", () => {
        for (const k of ["created", "branch", "author", "status"]) expect(skill.metadata.get(k), k).toBeTruthy();
        expect(skill.metadata.get("author")).toMatch(/Larry Klosowski/);
      });
      it("cites sources, and every cited repo is pinned in metadata by a full commit", () => {
        const cites = extractSkillCitations(skill.body);
        expect(cites.length).toBeGreaterThanOrEqual(3);
        for (const c of cites) expect(skill.metadata.get(c.source) ?? "", `${c.source} pin`).toMatch(/^[0-9a-f]{40}$/);
      });
      it("uses Citrate vocabulary: HIC, never the old term, and no em-dashes in prose", () => {
        expect(text).not.toMatch(/\bHITL\b/);
        expect(text).not.toMatch(/dogfood/i);
        expect(text).not.toContain("\u2014");
      });

      const cites = extractSkillCitations(skill.body);
      const repos = [...new Set(cites.map((c) => c.source))];
      const unavailable = repos.filter((r) => {
        const pin = skill.metadata.get(r) ?? "";
        return !existsSync(join(sourcesRoot, r)) || git(join(sourcesRoot, r), ["cat-file", "-e", `${pin}^{commit}`]) === null;
      });
      if (unavailable.length) {
        console.info(`${name}: live citation check skipped, ${unavailable.join(", ")} not found at the pinned commit under ${sourcesRoot}`);
      }
      it.skipIf(unavailable.length > 0)(
        "every cited path exists at its pinned commit, and every #symbol occurs in that file",
        () => {
          const problems: string[] = [];
          const cache = new Map<string, string | null>();
          for (const c of cites) {
            const pin = skill.metadata.get(c.source) ?? "";
            const key = `${c.source}:${c.path}`;
            if (!cache.has(key)) cache.set(key, git(join(sourcesRoot, c.source), ["show", `${pin}:${c.path}`]));
            const file = cache.get(key);
            if (file === null || file === undefined) problems.push(`${key} does not exist at ${pin}`);
            else if (c.symbol && !file.includes(c.symbol)) problems.push(`${key}#${c.symbol}: symbol not found`);
          }
          expect(problems).toEqual([]);
        },
        60_000,
      );
    });
  }
});

// Review fix: the 0x0110 worked example must be internally consistent, so a reader who copies it
// gets an input the precompile accepts and an output of the documented size.
describe("citrate-belnap-aggregate worked example", () => {
  const text = readFileSync(join(skillsRoot, "citrate-belnap-aggregate", "SKILL.md"), "utf8");
  const hexBlockAfter = (marker: RegExp): { header: RegExpMatchArray; hex: string } => {
    const header = text.match(marker);
    if (!header || header.index === undefined) throw new Error(`marker ${marker} not found`);
    const fence = text.indexOf("```text\n", header.index);
    const end = text.indexOf("\n```", fence + 8);
    const hex = text.slice(fence + 8, end).replace(/\s+/g, "").replace(/^0x/, "");
    return { header, hex };
  };

  it("declares the input's true byte length, which matches 24 + 16 n dim + 8 n", () => {
    const { header, hex } = hexBlockAfter(/Input \((\d+) bytes/);
    expect(hex).toMatch(/^[0-9a-f]+$/);
    const bytes = hex.length / 2;
    const dim = parseInt(hex.slice(0, 8), 16);
    const n = parseInt(hex.slice(8, 16), 16);
    expect(Number(header[1])).toBe(bytes);
    expect(bytes).toBe(24 + 16 * n * dim + 8 * n);
  });
  it("shows an output of exactly 9 * dim bytes: dim i64 values, then dim state bytes", () => {
    const input = hexBlockAfter(/Input \((\d+) bytes/).hex;
    const dim = parseInt(input.slice(0, 8), 16);
    const { hex } = hexBlockAfter(/Output returned by chain 40204/);
    expect(hex.length / 2).toBe(9 * dim);
    const states = hex.slice(16 * dim).match(/../g)?.map((b) => parseInt(b, 16));
    expect(states).toEqual([1, 3, 0, 1]);
  });
});
