// #493 / K6-3 tripwire (webview half; the Rust half is k6_no_chain_id_or_rpc_literal_outside_the_profile
// in src-tauri/src/network_profile_tests.rs). The chain id and the RPC come from the network
// profile (src/network/profile.ts → src-tauri/network/profiles.json) and from `citrate` in
// src/chain.ts, which reads it. A literal anywhere else keeps the old value when the profile
// changes, so this scans the production webview code and fails on:
// - the chain id as a number in code (40204, 40_204, 40204n, 0x9d0c);
// - the chain id as a whole string value ("40204", "0x9d0c", "eip155:40204");
// - the RPC host in any string ("rpc.citrate.ai", with or without a scheme).
// Comments and test files are skipped. Prose that names the chain ("chain 40204") is copy, not a
// value a path uses, and is not matched.
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { ACTIVE_PROFILE } from "./profile";
import { CITRATE_QA_SET } from "../agent/eval";

/**
 * Pinned copies, not derived: each is checked equal to the active profile below, so a profile
 * change fails here. `src/agent/eval.ts` is the answer-quality dataset (the fact the model's
 * answer must contain); it runs under plain Node (scripts/eval-qa.mjs), which cannot import the
 * profile JSON.
 */
const PINNED: { file: string; finding: string }[] = [{ file: "src/agent/eval.ts", finding: 'chain id string "40204"' }];

type Lexed = { code: string; strings: { at: number; text: string }[] };

/** Comments removed and string/template literals blanked (newlines kept), plus every literal. */
function lexTs(src: string): Lexed {
  let code = "";
  const strings: Lexed["strings"] = [];
  const blank = (c: string) => (c === "\n" ? "\n" : " ");
  let i = 0;
  while (i < src.length) {
    const c = src[i];
    const n = src[i + 1];
    if (c === "/" && n === "/") {
      while (i < src.length && src[i] !== "\n") {
        code += " ";
        i++;
      }
      continue;
    }
    if (c === "/" && n === "*") {
      const end = src.indexOf("*/", i + 2);
      const stop = end < 0 ? src.length : end + 2;
      for (; i < stop; i++) code += blank(src[i]);
      continue;
    }
    if (c === '"' || c === "'" || c === "`") {
      const at = code.length;
      code += " ";
      i++;
      let text = "";
      while (i < src.length && src[i] !== c) {
        if (src[i] === "\\" && i + 1 < src.length) {
          text += src[i] + src[i + 1];
          code += blank(src[i]) + blank(src[i + 1]);
          i += 2;
          continue;
        }
        if (c !== "`" && src[i] === "\n") break; // an unterminated quote: stop at the line end
        text += src[i];
        code += blank(src[i]);
        i++;
      }
      code += " ";
      i++;
      strings.push({ at, text });
      continue;
    }
    code += c;
    i++;
  }
  return { code, strings };
}

function intValue(tok: string): number | null {
  const t = tok.toLowerCase().replace(/_/g, "").replace(/n$/, "");
  if (/^0x[0-9a-f]+$/.test(t)) return parseInt(t.slice(2), 16);
  if (/^[0-9]+$/.test(t)) return Number(t);
  return null;
}

/** Each finding is "line: what". */
function networkLiteralsInTs(src: string): string[] {
  const { code, strings } = lexTs(src);
  const lineOf = (at: number) => code.slice(0, at).split("\n").length;
  const out: string[] = [];
  const num = /(?<![A-Za-z0-9_$.])(0[xX][0-9a-fA-F_]+n?|[0-9][0-9_]*n?)(?![A-Za-z0-9_$])/g;
  for (const m of code.matchAll(num)) {
    if (intValue(m[1]) !== 40204) continue;
    // A value in code follows an operator or punctuation (`chainId: 40204`, `=== 40204`,
    // `f(40204)`, `{40204}`); a number after a word is JSX text, which is copy.
    const before = code.slice(0, m.index ?? 0).trimEnd();
    if (/[:=(,[?!&|+\-*/{]$/.test(before) || /\breturn$/.test(before)) out.push(`${lineOf(m.index ?? 0)}: chain id ${m[1]}`);
  }
  for (const s of strings) {
    const t = s.text.toLowerCase();
    if (t === "40204" || t === "0x9d0c" || t.includes("eip155:40204")) out.push(`${lineOf(s.at)}: chain id string ${JSON.stringify(s.text)}`);
    if (t.includes("rpc.citrate.ai")) out.push(`${lineOf(s.at)}: RPC host ${JSON.stringify(s.text)}`);
  }
  return out;
}

function productionFiles(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) {
      if (name !== "__tests__") productionFiles(p, out);
    } else if (/\.(ts|tsx)$/.test(name) && !/\.test\.(ts|tsx)$/.test(name) && !name.endsWith(".d.ts")) {
      out.push(p);
    }
  }
  return out;
}

describe("no chain id or RPC literal outside the network profile (#493, K6-3)", () => {
  it("the scan finds each form and skips comments and prose", () => {
    const src = [
      "// 40204 and https://rpc.citrate.ai in a comment", // 1
      "/* 40204 */", // 2
      "const a = { chainId: 40204 };", // 3
      "const b = 40_204;", // 4
      "const c = 0x9d0c;", // 5
      'const d = "0x9d0c";', // 6
      "const e = 'https://rpc.citrate.ai';", // 7
      "const f = `rpc.citrate.ai · live`;", // 8
      'const g = "not deployed on chain 40204 yet";', // 9
      "const h = 402040;", // 10
      'if (t === "40204") {}', // 11
      "const i = `eip155:40204/tx/${h}`;", // 12
      "const j = <div>Syncing chain 40204</div>;", // 13 (JSX text is code to the scan)
      "const k = <Card id={40204} />;", // 14
      "if (id === 40204) {}", // 15
      "switch (x) { case 40204: break; }", // 16
      "const l = () => 40204;", // 17
      "if (x < 40204 || x > 40204) {}", // 18
      "const m = `chain ${40204}`;", // 19
      `const n = '{"chainId":40204}';`, // 20
      'const o = "http://rpc.citrate.ai";', // 21
      `const p = '{"chain_id": "0x9d0c"}';`, // 22
      "const q = `a ${`b ${40_204}`} c`;", // 23 (nested template)
      "const r = 140204 + 402041;", // 24 (other numbers)
    ].join("\n");
    const lines = [...new Set(networkLiteralsInTs(src).map((f) => Number(f.split(":")[0])))].sort((a, b) => a - b);
    expect(lines).toEqual([3, 4, 5, 6, 7, 8, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23]);
  });

  it("the production webview code holds none", () => {
    const root = join(__dirname, "..");
    const files = productionFiles(root);
    expect(files.length, "positive control: the scan reads the webview").toBeGreaterThan(100);
    const offenders: string[] = [];
    for (const f of files) {
      const rel = `src/${relative(root, f)}`;
      for (const hit of networkLiteralsInTs(readFileSync(f, "utf8"))) {
        if (PINNED.some((p) => p.file === rel && hit.endsWith(": " + p.finding))) continue;
        offenders.push(`${rel}:${hit}`);
      }
    }
    expect(offenders, "read the chain id and RPC from src/network/profile.ts (or `citrate` in src/chain.ts)").toEqual([]);
  });

  it("the pinned eval fact is the active profile's chain id", () => {
    const item = CITRATE_QA_SET.find((q) => q.id === "chain-id");
    expect(item?.expectFacts).toEqual([String(ACTIVE_PROFILE?.chainId)]);
  });
});
