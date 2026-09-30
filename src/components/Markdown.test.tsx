// HUP-S0.4 — golden tests for the agent's markdown (owner report 2026-09-29: "the text is bad in the
// agent"). Cases are the shapes the model actually emits: snake_case tool names, file names with
// underscores, bold, tables, nested lists, numbered lists split by blank lines, and fences with
// language tags like c++ / shell-session.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Markdown } from "./Markdown";

const html = (md: string) => renderToStaticMarkup(<Markdown text={md} />);

describe("HUP-S0.4 Markdown", () => {
  it("never italicizes inside snake_case identifiers or file names", () => {
    const out = html("Use node_status and staking_status, then load gemma-4-E4B-it-Q4_0.gguf.");
    expect(out).not.toContain("<em>");
    expect(out).toContain("node_status");
    expect(out).toContain("staking_status");
    expect(out).toContain("Q4_0.gguf");
  });

  it("still italicizes real emphasis", () => {
    expect(html("this is _really_ it")).toContain("<em>really</em>");
    expect(html("this is *really* it")).toContain("<em>really</em>");
  });

  it("renders bold, including bold italic, with no stray asterisks", () => {
    expect(html("**Staking** locks SALT")).toContain("<strong>Staking</strong>");
    const bi = html("***important***");
    expect(bi).toContain("<strong>");
    expect(bi).toContain("<em>");
    expect(bi).not.toContain("*");
  });

  it("does not treat 2 * 3 * 4 arithmetic as emphasis", () => {
    expect(html("2 * 3 * 4 = 24")).not.toContain("<em>");
  });

  it("renders GFM tables", () => {
    const out = html("| Tool | Kind |\n|---|:---:|\n| node_status | read |\n| group_invite | write |");
    expect(out).toContain("<table");
    expect(out).toContain("<th");
    expect(out.match(/<tr/g)?.length).toBe(3);
    expect(out).toContain("node_status");
    expect(out).not.toContain("|---|");
  });

  it("nests indented list items", () => {
    const out = html("- Wallet\n  - Send\n  - Stake\n- Node");
    expect(out.match(/<ul/g)?.length).toBe(2);
    expect(out).toMatch(/<li[^>]*>Wallet<ul/);
  });

  it("keeps one ordered list across blank lines and honours the start number", () => {
    const one = html("1. First\n\n2. Second\n\n3. Third");
    expect(one.match(/<ol/g)?.length).toBe(1);
    expect(one.match(/<li/g)?.length).toBe(3);
    expect(html("4. Fourth\n5. Fifth")).toContain('start="4"');
  });

  it("recognizes fences with any language tag, tildes, and indentation", () => {
    for (const md of ["```c++\nint x;\n```", "```shell-session\n$ ls\n```", "~~~\nplain\n~~~", "  ```js\nlet a\n  ```"]) {
      const out = html(md);
      expect(out, md).toContain("<pre");
      expect(out, md).not.toContain("```");
      expect(out, md).not.toContain("~~~");
    }
  });

  it("an unclosed fence (mid-stream) renders as code, not literal backticks", () => {
    const out = html("Here:\n```solidity\ncontract A {");
    expect(out).toContain("<pre");
    expect(out).toContain("contract A {");
  });

  it("renders strikethrough", () => {
    expect(html("~~old~~ new")).toContain("<del>old</del>");
  });

  it("never emits raw HTML from model text", () => {
    const out = html("<img src=x onerror=alert(1)> and [x](javascript:alert(1))");
    expect(out).not.toContain("<img");
    expect(out).not.toContain('href="javascript');
  });
});

describe("HUP-S0.4 Markdown termination (the ```c++ OOM class)", () => {
  it("always terminates on random markdown-heavy input", () => {
    const alphabet = ["`", "```", "~~~", "c++", "*", "**", "_", "#", "- ", "1. ", "|", "---", ">", "  ", "\n", "\n\n", "a", "x_y", " "];
    let seed = 1;
    const rnd = () => (seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff;
    for (let n = 0; n < 400; n++) {
      let s = "";
      const len = 1 + Math.floor(rnd() * 40);
      for (let j = 0; j < len; j++) s += alphabet[Math.floor(rnd() * alphabet.length)];
      const t0 = Date.now();
      html(s);
      expect(Date.now() - t0, JSON.stringify(s)).toBeLessThan(200);
    }
  });
});
