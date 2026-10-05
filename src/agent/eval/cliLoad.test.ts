// @vitest-environment node
//
// The eval CLIs run under plain Node with its built-in type stripping (no bundler), so every
// module they reach must import siblings with an explicit `.ts` extension. A bare `./knowledgeSearch`
// in harness.ts once left eval-tools, eval-sidecar and eval-qa unable to start (ERR_MODULE_NOT_FOUND)
// while every vitest run stayed green. This loads each CLI for real: with no arguments it must get
// as far as its own usage error (exit 2), which proves the whole import graph resolved.
import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const ROOT = process.cwd();
// The CLIs need Node's type stripping (Node >= 22.18 / 23.6, as the eval README says).
const strips = Boolean((process.features as { typescript?: unknown }).typescript);
if (!strips) console.info(`eval CLI load check skipped: Node ${process.version} does not strip TypeScript types`);

describe.skipIf(!strips)("eval CLIs load under plain Node", () => {
  for (const script of ["eval-tools.mjs", "eval-sidecar.mjs", "eval-qa.mjs"]) {
    it(`${script} reaches its usage error`, () => {
      const r = spawnSync(process.execPath, [resolve(ROOT, "scripts", script)], { cwd: ROOT, encoding: "utf8", timeout: 60_000 });
      expect(r.stderr).not.toMatch(/ERR_MODULE_NOT_FOUND|ERR_UNSUPPORTED|SyntaxError/);
      expect(r.stderr).toMatch(/--base-url is required/);
      expect(r.status).toBe(2);
    }, 60_000);
  }
});
