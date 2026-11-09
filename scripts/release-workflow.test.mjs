// @vitest-environment node
//
// Lint of .github/workflows/release.yml beyond the pins (component-pins.mjs check covers staging,
// the dry-run guards and the GitHub-release upload ban). Each case is one review finding on #608:
// the chain read token never reaches a step that builds third-party code, the tag is checked
// before anything is signed, a manual release builds the tag it names, the workflow artifacts are
// short-lived, and each job writes its updater-feed fragment. Plain text, no YAML dependency.
import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { workflowSteps } from "./component-pins.mjs";

const repo = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const wf = fs.readFileSync(path.join(repo, ".github", "workflows", "release.yml"), "utf8");
const ci = fs.readFileSync(path.join(repo, ".github", "workflows", "ci.yml"), "utf8");

/** { name: text } for each job under `jobs:` (a job starts at a two-space `name:` key). */
function jobs(text) {
  const body = text.slice(text.indexOf("\njobs:\n") + 7);
  const out = {};
  const re = /^ {2}([a-z0-9-]+):\n/gm;
  const marks = [...body.matchAll(re)];
  marks.forEach((m, i) => {
    out[m[1]] = body.slice(m.index, i + 1 < marks.length ? marks[i + 1].index : body.length);
  });
  return out;
}
const J = jobs(wf);
const NAMES = ["release-macos-arm", "release-linux-aarch64"];
const stepName = (s) => (/name: (.*)/.exec(s) || [])[1] || s.split("\n")[0].trim();
const SIGNING = /secrets\.(TAURI_SIGNING|APPLE_|KEYCHAIN)/;
const CHECKOUT_REF = "ref: ${{ github.event_name == 'workflow_dispatch' && inputs.dry_run != true && format('refs/tags/{0}', inputs.tag) || github.ref }}";

describe("release.yml jobs", () => {
  it("has exactly the macOS and Linux aarch64 jobs", () => {
    expect(Object.keys(J).sort()).toEqual([...NAMES].sort());
  });

  for (const name of NAMES) {
    describe(name, () => {
      const text = () => J[name];
      const steps = () => workflowSteps(text());

      it("reads no secret in its job-level env", () => {
        const env = /^ {4}env:\n((?: {6}.*\n)+)/m.exec(text());
        expect(env, "job env").not.toBeNull();
        expect(env[1]).not.toMatch(/secrets\./);
      });

      it("hands the chain read token only to steps that clone, never to a build", () => {
        const withToken = steps().filter((s) => s.includes("CITRATE_CHAIN_READ_TOKEN"));
        for (const s of withToken) {
          expect(stepName(s)).toMatch(/^Clone /);
          expect(s).not.toMatch(/cargo |build-component|build-sidecar|build-hermes|tauri build|npm |npx /);
        }
        expect(text()).not.toMatch(/git config --global/);
      });

      it("checks the tag right after checkout, before any step that reads a signing secret", () => {
        const s = steps();
        const check = s.findIndex((x) => stepName(x) === "Check the tag");
        expect(check).toBe(1);
        expect(s[check]).toContain("^v[0-9]+\\.[0-9]+\\.[0-9]+$");
        expect(s[check]).toMatch(/tauri\.conf\.json/);
        expect(s[check]).not.toMatch(/\$\{\{/);
        const firstSecret = s.findIndex((x) => SIGNING.test(x));
        if (firstSecret >= 0) expect(firstSecret).toBeGreaterThan(check);
      });

      it("checks out the tag a manual release names, not the dispatch ref", () => {
        expect(steps()[0]).toContain("actions/checkout@");
        expect(steps()[0]).toContain(CHECKOUT_REF);
      });

      it("can read, not write, the repository", () => {
        expect(text()).toMatch(/^ {4}permissions:\n {6}contents: read\n/m);
      });

      it("keeps its workflow artifacts at most two days", () => {
        const days = [...text().matchAll(/retention-days: (\d+)/g)].map((m) => Number(m[1]));
        expect(days.length).toBeGreaterThan(0);
        for (const d of days) expect(d).toBeLessThanOrEqual(2);
      });
    });
  }

  it("each job writes its own updater-feed fragment", () => {
    expect(J["release-macos-arm"]).toMatch(/updater-feed\.mjs fragment [^\n]*(\\\n[^\n]*)*darwin-aarch64/);
    expect(J["release-linux-aarch64"]).toMatch(/updater-feed\.mjs fragment [^\n]*(\\\n[^\n]*)*linux-aarch64 linux-aarch64-deb/);
  });
});

describe("ci.yml", () => {
  // The RUSTSEC-2026-0387 ignore expires on a date: a repo with no pushes must still run the
  // guard, or the expiry passes unseen.
  it("runs on a weekly schedule as well as on pushes and pull requests", () => {
    expect(ci).toMatch(/^ {2}schedule:\n {4}- cron: "\d+ \d+ \* \* [0-6]"$/m);
    expect(ci).toContain("python3 scripts/check-crypto-bigint-users.py");
  });
});
