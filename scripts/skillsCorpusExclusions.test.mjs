// @vitest-environment node
//
// 0.5.4, owner decision 2026-11-16 (Hermes x FL collection policy): sherlock, unbroker and
// osint-investigation (people search and OSINT about individuals) and scrapling (scraping around
// the compliant collector) are not in the bundled Hermes corpus. The bundle ships as the pinned
// skills-bundle runtime-deps asset, built and verified from skills.lock, so the lock is what
// decides: a staged bundle that carries any of them fails `stage-skills-bundle.mjs verify`.
import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";
import { parseLockToml } from "./stage-skills-bundle.mjs";

const ROOT = path.resolve(import.meta.dirname, "..");
const lock = parseLockToml(fs.readFileSync(path.join(ROOT, "skills.lock"), "utf8"));
const intake = JSON.parse(fs.readFileSync(path.join(ROOT, ".agentile/skill-intake/intake.json"), "utf8"));
const SHIPS = new Set(["include-as-is", "include-with-scripts-stripped", "convert-script-to-capsule"]);
const EXCLUDED = [
  ["hermes-optional", "security/sherlock"],
  ["hermes-optional", "security/unbroker"],
  ["hermes-optional", "research/osint-investigation"],
  ["hermes-optional", "research/scrapling"],
];

describe("the Hermes corpus leaves out people search, OSINT and scraping around the collector", () => {
  it.each(EXCLUDED)("%s/%s is excluded in the intake and does not ship in skills.lock", (source, p) => {
    const d = intake.decisions[`${source}/${p}`];
    expect(d?.verdict).toBe("exclude");
    expect(d.reason).toMatch(/owner decision 2026-11-16/);
    const entry = lock.skills.find((s) => s.source === source && s.path === p);
    expect(entry?.verdict ?? "exclude").toBe("exclude");
    expect(lock.skills.filter((s) => SHIPS.has(s.verdict)).some((s) => s.source === source && s.path === p)).toBe(false);
  });

  it("no shipping skill carries one of those names", () => {
    const names = new Set(lock.skills.filter((s) => SHIPS.has(s.verdict)).map((s) => s.name));
    for (const n of ["sherlock", "unbroker", "osint-investigation", "scrapling"]) expect(names.has(n)).toBe(false);
  });

  it("a staged bundle in this checkout, if there is one, holds none of them", () => {
    const staged = path.join(ROOT, "src-tauri/skills-bundle");
    for (const [source, p] of EXCLUDED) expect(fs.existsSync(path.join(staged, source, p))).toBe(false);
  });
});
