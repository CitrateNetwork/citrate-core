// @vitest-environment node
//
// HUP-S3.2: the reviewed third-party skills ship as a bundle resource (src-tauri/skills-bundle/)
// that the sidecar loads as a locked source. The stager copies exactly what skills.lock admits,
// checks every byte against the lock, applies the recorded intake rewrite, and refuses anything
// else; `verify` re-checks a staged tree (the release step runs it on the runtime-deps asset).
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { buildLock, renderLock } from "./skills-lock.mjs";
import { flattenFrontmatter } from "./skill-intake-rewrite.mjs";
import { parseLockToml, stageFromSources, verifyStaged } from "./stage-skills-bundle.mjs";

const sha = (b) => createHash("sha256").update(b).digest("hex");
let tmp;
const w = (rel, text) => {
  const p = path.join(tmp, rel);
  fs.mkdirSync(path.dirname(p), { recursive: true });
  fs.writeFileSync(p, text);
};

const NESTED =
  "---\nname: arxiv\ndescription: Search arXiv papers.\nmetadata:\n  hermes:\n    tags: [Research]\n---\nProse.\n";

const intake = {
  sources: [
    {
      label: "fixture",
      upstream: "https://example.invalid/fixture",
      commit: "0123456789abcdef0123456789abcdef01234567",
      license: "MIT",
      local: "src/fixture",
      pin_method: "vendored-snapshot",
    },
    {
      label: "forked",
      upstream: "https://example.invalid/forked",
      commit: "89abcdef0123456789abcdef0123456789abcdef",
      license: "MIT",
      local: "src/forked",
      pin_method: "git-checkout",
      intake_rewrite: "flatten-frontmatter",
    },
  ],
  decisions: {
    "fixture/gamma": { verdict: "exclude", reason: "personality analysis" },
  },
};

let lockText;
beforeAll(() => {
  tmp = fs.mkdtempSync(path.join(os.tmpdir(), "stage-skills-"));
  w("src/fixture/alpha/SKILL.md", "---\nname: alpha\ndescription: Alpha.\n---\nRead the checklist.\n");
  w("src/fixture/alpha/references/checklist.md", "- one\n");
  w("src/fixture/alpha/scripts/run.py", "print(1)\n");
  w("src/fixture/gamma/SKILL.md", "---\nname: gamma\ndescription: Excluded.\n---\nx\n");
  w("src/forked/research/arxiv/SKILL.md", NESTED);
  lockText = renderLock(buildLock(intake, path.join(tmp)));
  w("repo/skills.lock", lockText);
});
afterAll(() => {
  if (tmp) fs.rmSync(tmp, { recursive: true, force: true });
});

describe("parseLockToml", () => {
  it("reads the generated lock back", () => {
    const lock = parseLockToml(lockText);
    expect(lock.sources.map((s) => s.label)).toEqual(["fixture", "forked"]);
    const arxiv = lock.skills.find((s) => s.name === "arxiv");
    expect(arxiv.intake_rewrite).toBe("flatten-frontmatter");
    expect(arxiv.shipped_skill_md_sha256).toBe(sha(flattenFrontmatter(NESTED)));
    const alpha = lock.skills.find((s) => s.name === "alpha");
    expect(alpha.refs).toEqual([{ path: "references/checklist.md", sha256: sha("- one\n") }]);
    expect(alpha.stripped).toEqual(["scripts/run.py"]);
  });

  it("refuses a line it does not understand", () => {
    expect(() => parseLockToml("version = 1\nsomething odd\n")).toThrow(/line 2/);
  });
});

describe("stageFromSources", () => {
  it("copies exactly the admitted files, rewrites what the lock says, and copies the lock", () => {
    const out = path.join(tmp, "out1");
    fs.mkdirSync(out);
    fs.writeFileSync(path.join(out, "README.md"), "kept\n");
    fs.writeFileSync(path.join(out, "stale.txt"), "removed\n");
    const r = stageFromSources(lockText, tmp, out, intake.sources);
    expect(r.skills).toBe(2);
    const files = [];
    const walk = (d) => {
      for (const e of fs.readdirSync(d, { withFileTypes: true })) {
        const p = path.join(d, e.name);
        if (e.isDirectory()) walk(p);
        else files.push(path.relative(out, p).split(path.sep).join("/"));
      }
    };
    walk(out);
    expect(files.sort()).toEqual([
      "README.md",
      "fixture/alpha/SKILL.md",
      "fixture/alpha/references/checklist.md",
      "forked/research/arxiv/SKILL.md",
      "skills.lock",
    ]);
    expect(fs.readFileSync(path.join(out, "forked/research/arxiv/SKILL.md"), "utf8")).toBe(flattenFrontmatter(NESTED));
    expect(fs.readFileSync(path.join(out, "skills.lock"), "utf8")).toBe(lockText);
    expect(verifyStaged(lockText, out)).toEqual({ ok: true, problems: [] });
  });

  it("refuses a source file that drifted from the lock", () => {
    w("src/fixture/alpha/references/checklist.md", "- one!\n");
    try {
      expect(() => stageFromSources(lockText, tmp, path.join(tmp, "out2"), intake.sources)).toThrow(/checklist\.md/);
    } finally {
      w("src/fixture/alpha/references/checklist.md", "- one\n");
    }
  });
});

describe("verifyStaged", () => {
  const staged = () => {
    const out = fs.mkdtempSync(path.join(tmp, "v-"));
    stageFromSources(lockText, tmp, out, intake.sources);
    return out;
  };

  it("refuses an extra file, a script, a changed file, a missing skill and a symlink", () => {
    let out = staged();
    fs.writeFileSync(path.join(out, "fixture/alpha/scripts.sh"), "x");
    expect(verifyStaged(lockText, out).problems.join("\n")).toMatch(/scripts\.sh is not pinned/);

    out = staged();
    fs.mkdirSync(path.join(out, "fixture/alpha/scripts"));
    fs.writeFileSync(path.join(out, "fixture/alpha/scripts/run.py"), "print(1)\n");
    expect(verifyStaged(lockText, out).ok).toBe(false);

    out = staged();
    fs.writeFileSync(path.join(out, "fixture/alpha/SKILL.md"), "tampered");
    expect(verifyStaged(lockText, out).problems.join("\n")).toMatch(/alpha\/SKILL\.md/);

    out = staged();
    fs.rmSync(path.join(out, "forked"), { recursive: true });
    expect(verifyStaged(lockText, out).problems.join("\n")).toMatch(/arxiv/);

    out = staged();
    fs.rmSync(path.join(out, "fixture/alpha/references/checklist.md"));
    fs.symlinkSync("/etc/hosts", path.join(out, "fixture/alpha/references/checklist.md"));
    expect(verifyStaged(lockText, out).problems.join("\n")).toMatch(/symlink/);
  });

  it("refuses a staged lock that differs from the repo's", () => {
    const out = staged();
    fs.writeFileSync(path.join(out, "skills.lock"), lockText + "\n");
    expect(verifyStaged(lockText, out).problems.join("\n")).toMatch(/skills\.lock/);
  });
});
