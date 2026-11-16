// created: 2026-11-12 | branch: fix/053-case-collision
// author: Larry Klosowski (MAC lane, CORE-053; Claude Opus 5.5)
// status: active
//
// The release Mac builds on a case-insensitive file system. Two modules whose import paths differ
// only in case (src/surfaces/ClusterMap.tsx and clusterMap.ts, #634) resolve to the same file
// there, so `tsc` fails on macOS (TS1261) while Linux CI passes. The 0.5.3 macOS release build
// stopped on exactly that. No two tracked files may have import paths that differ only in case.
import { describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const files = execFileSync("git", ["ls-files", "-z"], { cwd: repo, encoding: "utf8" }).split("\0").filter(Boolean);

describe("no import paths that differ only in case (macOS builds)", () => {
  it("tracked paths, with and without a script extension, are unique ignoring case", () => {
    const strip = (f) => f.replace(/\.(d\.ts|tsx?|jsx?|mjs|cjs)$/, "");
    const seen = new Map();
    const clashes = [];
    for (const f of files) {
      for (const k of new Set([f, strip(f)])) {
        const low = k.toLowerCase();
        const other = seen.get(low);
        if (other && other.key !== k) clashes.push(`${other.file} <-> ${f}`);
        else if (!other) seen.set(low, { key: k, file: f });
      }
    }
    expect(clashes).toEqual([]);
  });
});
