// Tripwire (0.5.2, C1-V chain reads): no module under src/ may build a viem client while it is
// being imported. viem checks the RPC url when a client is built, so a client built at import
// time turns "this build has no RPC url" into a module that cannot load, and the app cannot show
// its not-ready state. The store singleton did this through a class field (`createChainReads()`).
//
// The scan parses every non-test file under src/ and fails on:
//   1. a client constructor (createPublicClient, createWalletClient, createTestClient, createClient)
//      called at module scope, in a class field initializer or in a class static block;
//   2. a named function that builds a client in its own body (not in a nested callback), called
//      from one of those import-time places. That is the one-step indirection #348 used.
import { describe, it, expect } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import ts from "typescript";

const SRC = join(__dirname, "..");
const CLIENT_CTORS = new Set(["createPublicClient", "createWalletClient", "createTestClient", "createClient"]);

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    if (n === "node_modules") return [];
    if (statSync(p).isDirectory()) return files(p);
    return /\.(ts|tsx)$/.test(n) && !/\.test\.(ts|tsx)$/.test(n) && !/\.d\.ts$/.test(n) ? [p] : [];
  });
}

function calleeName(call: ts.CallExpression): string | null {
  const e = call.expression;
  if (ts.isIdentifier(e)) return e.text;
  if (ts.isPropertyAccessExpression(e)) return e.name.text;
  return null;
}

/** The innermost function around `node`, or null when it runs at import time. */
function importTimeContext(node: ts.Node): { importTime: boolean; fn: ts.FunctionLikeDeclaration | null } {
  for (let p = node.parent; p; p = p.parent) {
    if (ts.isFunctionLike(p) && !ts.isConstructorDeclaration(p)) return { importTime: false, fn: p as ts.FunctionLikeDeclaration };
    // A field initializer and a static block run when the class is instantiated or defined;
    // for a module singleton (`export const store = new Store()`) that is import time.
    if (ts.isPropertyDeclaration(p) && p.initializer && node.pos >= p.initializer.pos && node.end <= p.initializer.end)
      return { importTime: true, fn: null };
    if (ts.isClassStaticBlockDeclaration(p)) return { importTime: true, fn: null };
    if (ts.isConstructorDeclaration(p)) return { importTime: true, fn: null };
  }
  return { importTime: true, fn: null };
}

function fnName(fn: ts.FunctionLikeDeclaration): string | null {
  if ((ts.isFunctionDeclaration(fn) || ts.isMethodDeclaration(fn)) && fn.name && ts.isIdentifier(fn.name)) return fn.name.text;
  if ((ts.isArrowFunction(fn) || ts.isFunctionExpression(fn)) && ts.isVariableDeclaration(fn.parent) && ts.isIdentifier(fn.parent.name))
    return fn.parent.name.text;
  return null;
}

interface Parsed {
  rel: string;
  sf: ts.SourceFile;
}

function scan(parsed: Parsed[]): string[] {
  const offenders: string[] = [];
  const eagerFactories = new Set<string>();
  const at = (p: Parsed, n: ts.Node) => `${p.rel}:${p.sf.getLineAndCharacterOfPosition(n.getStart(p.sf)).line + 1}`;

  // Pass 1: client constructors at import time; collect the named functions that build one directly.
  for (const p of parsed) {
    const visit = (n: ts.Node) => {
      if (ts.isCallExpression(n)) {
        const name = calleeName(n);
        if (name && CLIENT_CTORS.has(name)) {
          const ctx = importTimeContext(n);
          if (ctx.importTime) offenders.push(`${at(p, n)}: ${name}() runs at import time`);
          else if (ctx.fn) {
            const f = fnName(ctx.fn);
            if (f) eagerFactories.add(f);
          }
        }
      }
      ts.forEachChild(n, visit);
    };
    visit(p.sf);
  }

  // Pass 2: those factories called at import time.
  for (const p of parsed) {
    const visit = (n: ts.Node) => {
      if (ts.isCallExpression(n)) {
        const name = calleeName(n);
        if (name && eagerFactories.has(name) && importTimeContext(n).importTime)
          offenders.push(`${at(p, n)}: ${name}() builds a viem client and runs at import time`);
      }
      ts.forEachChild(n, visit);
    };
    visit(p.sf);
  }
  return offenders;
}

function parse(rel: string, text: string): Parsed {
  return { rel, sf: ts.createSourceFile(rel, text, ts.ScriptTarget.Latest, true, rel.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS) };
}

describe("tripwire: no viem client is built at import time", () => {
  it("no module under src/ builds a viem client at module scope, in a class field, or via a factory called there", () => {
    const parsed = files(SRC).map((f) => parse(relative(SRC, f).split("\\").join("/"), readFileSync(f, "utf8")));
    expect(scan(parsed)).toEqual([]);
  });

  it("the scan catches each import-time shape and passes the deferred ones", () => {
    const bad = parse(
      "bad.ts",
      [
        'import { createPublicClient, http } from "viem";',
        "const top = createPublicClient({ transport: http() });",
        "function makeReads() { const c = createPublicClient({ transport: http() }); return { c }; }",
        "class S { reads = makeReads(); direct = createPublicClient({ transport: http() }); }",
        "export const s = new S();",
        "const viaTop = makeReads();",
      ].join("\n"),
    );
    expect(scan([bad])).toEqual([
      "bad.ts:2: createPublicClient() runs at import time",
      "bad.ts:4: createPublicClient() runs at import time",
      "bad.ts:4: makeReads() builds a viem client and runs at import time",
      "bad.ts:6: makeReads() builds a viem client and runs at import time",
    ]);
    const good = parse(
      "good.ts",
      [
        'import { createPublicClient, http } from "viem";',
        "function lazyReads() { let c: unknown = null; const get = () => (c ??= createPublicClient({ transport: http() })); return { get }; }",
        "class S { reads = lazyReads(); probe() { return createPublicClient({ transport: http() }); } }",
        "export const s = new S();",
        "export function onClick() { return createPublicClient({ transport: http() }); }",
      ].join("\n"),
    );
    expect(scan([good])).toEqual([]);
  });
});
