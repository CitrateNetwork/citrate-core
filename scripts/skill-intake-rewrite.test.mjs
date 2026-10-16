// @vitest-environment node
//
// HUP-S3.2 (US-3.2 AC2): the strict SKILL.md loader stays strict. A third-party skill whose
// frontmatter uses nested maps or keys outside the loader's set (the hermes-agent fork) is
// rewritten at intake: the frontmatter is flattened into `metadata` strings, the body is left
// byte for byte, and the result must pass the same strict check the runtime loader applies.
import { describe, expect, it } from "vitest";
import { flattenFrontmatter, INTAKE_REWRITE } from "./skill-intake-rewrite.mjs";
import { parseSkillMd } from "./skills-lock.mjs";

const ARXIV = `---
name: arxiv
description: "Search arXiv papers by keyword, author, category, or ID."
version: 1.0.0
author: Hermes Agent
license: MIT
platforms: [linux, macos, windows]
metadata:
  hermes:
    tags: [Research, Arxiv, Papers, Academic, Science, API]
    related_skills: [ocr-and-documents]
---

# arXiv Research

Body with --- inside a line and \`code\`.
`;

describe("flattenFrontmatter", () => {
  it("is named for the lock", () => {
    expect(INTAKE_REWRITE).toBe("flatten-frontmatter");
  });

  it("turns a nested hermes frontmatter into one the strict loader accepts", () => {
    expect(() => parseSkillMd(ARXIV)).toThrow(/nested collections/);
    const out = flattenFrontmatter(ARXIV);
    const { fm, body } = parseSkillMd(out);
    expect(fm.name).toBe("arxiv");
    expect(fm.description).toBe("Search arXiv papers by keyword, author, category, or ID.");
    expect(fm.license).toBe("MIT");
    expect(fm.extensions.version).toBe("1.0.0");
    expect(fm.metadata).toEqual({
      author: "Hermes Agent",
      platforms: "linux, macos, windows",
      "hermes-tags": "Research, Arxiv, Papers, Academic, Science, API",
      "hermes-related_skills": "ocr-and-documents",
      "intake-rewrite": "flatten-frontmatter",
    });
    expect(body).toBe(ARXIV.slice(ARXIV.indexOf("\n---\n") + 5));
  });

  it("is deterministic", () => {
    expect(flattenFrontmatter(ARXIV)).toBe(flattenFrontmatter(ARXIV));
  });

  it("flattens block lists, lists of maps and block scalars", () => {
    const src = `---
name: one-password
description: >
  Use the 1Password CLI
  to read secrets.
platforms: [macos]
required_environment_variables:
  - name: OP_SERVICE_ACCOUNT_TOKEN
    prompt: "1Password Service Account Token"
    secret: true
metadata:
  hermes:
    tags:
      - security
      - secrets
    credits: |
      Adapted from a
      community skill.
---
Body.
`;
    const { fm } = parseSkillMd(flattenFrontmatter(src));
    expect(fm.description).toBe("Use the 1Password CLI to read secrets.");
    expect(fm.metadata["hermes-tags"]).toBe("security, secrets");
    expect(fm.metadata["hermes-credits"]).toBe("Adapted from a\ncommunity skill.");
    expect(JSON.parse(fm.metadata.required_environment_variables)).toEqual([
      { name: "OP_SERVICE_ACCOUNT_TOKEN", prompt: "1Password Service Account Token", secret: "true" },
    ]);
  });

  it("leaves a skill the loader already accepts unchanged in meaning and marks the rewrite", () => {
    const ok = "---\nname: plain\ndescription: Plain skill.\n---\nBody.\n";
    const { fm, body } = parseSkillMd(flattenFrontmatter(ok));
    expect(fm.name).toBe("plain");
    expect(body).toBe("Body.\n");
    expect(fm.metadata["intake-rewrite"]).toBe("flatten-frontmatter");
  });

  it("refuses what it cannot represent faithfully (fail closed)", () => {
    expect(() => flattenFrontmatter("no frontmatter")).toThrow(/frontmatter/);
    expect(() => flattenFrontmatter("---\nname: a\n")).toThrow(/closing/);
    expect(() => flattenFrontmatter("---\nname: a\ndescription: d\nx: &anchor 1\n---\nb")).toThrow(/anchor/);
    const long = "---\nname: a\ndescription: d\nmetadata:\n  hermes:\n    tags: [" + "t, ".repeat(600) + "t]\n---\nb";
    expect(() => flattenFrontmatter(long)).toThrow(/1024/);
    expect(() => flattenFrontmatter("---\nname: a\ndescription: d\nname: b\n---\nb")).toThrow(/twice/);
  });
});
