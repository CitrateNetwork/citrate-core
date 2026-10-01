// HUP-S2.4 — approval cards generated from tool annotations. One card type per effect shape: a diff
// for file writes, the SignatureCeremony's decoded calldata for chain calls, the exact argv for
// commands, and named fields otherwise; every card leads with one plain-language summary line.
import { describe, it, expect } from "vitest";
import { lineDiff, diffCard, chainCard, commandCard, fieldsCard, argvOf, summaryLine } from "./approvalCards";
import type { CeremonyView } from "../bridge/types";

const write = { effect: "write", trust: "trusted" } as const;
const sign = { effect: "sign", trust: "trusted" } as const;

describe("Feature: diff card for a file write", () => {
  it("Given an existing file and new content, when the card is built, then it shows removed and added lines in order", () => {
    const d = lineDiff("a\nb\nc", "a\nB\nc\nd");
    expect(d).toEqual([
      { op: "context", text: "a" },
      { op: "remove", text: "b" },
      { op: "add", text: "B" },
      { op: "context", text: "c" },
      { op: "add", text: "d" },
    ]);
  });

  it("Given no previous file, then every line is an addition and the card says it creates the file", () => {
    const c = diffCard("skill_write", write, "skills/daily.md", "", "step 1\nstep 2");
    expect(c.kind).toBe("diff");
    if (c.kind !== "diff") return;
    expect(c.created).toBe(true);
    expect(c.lines.every((l) => l.op === "add")).toBe(true);
    expect(c.summary).toMatch(/create/i);
    expect(c.summary).toContain("skills/daily.md");
    expect(c.summary).toMatch(/2 lines added/);
  });

  it("Given a replacement, then the summary counts both sides", () => {
    const c = diffCard("skill_write", write, "skills/daily.md", "x\ny", "x\nz");
    expect(c.summary).toMatch(/1 line added, 1 removed/);
    if (c.kind === "diff") expect(c.created).toBe(false);
  });

  it("Given very large inputs, then the diff stays bounded and still lists every line once", () => {
    const before = Array.from({ length: 2000 }, (_, i) => "a" + i).join("\n");
    const after = Array.from({ length: 2000 }, (_, i) => "b" + i).join("\n");
    const d = lineDiff(before, after);
    expect(d.filter((l) => l.op === "remove")).toHaveLength(2000);
    expect(d.filter((l) => l.op === "add")).toHaveLength(2000);
  });
});

describe("Feature: chain card from the SignatureCeremony decoder", () => {
  const view: CeremonyView = {
    id: "cer-1",
    origin: "agent:hermes",
    kind: "transaction",
    chainId: 40204,
    decoded: { action: "Deploy contract (412 bytes)", destination: "contract creation", cost: "≈0.002 SALT gas" },
    requiresRawAck: false,
  };

  it("Given a decoded pending ceremony, then the card shows the decoder's action, destination, cost, chain and origin", () => {
    const c = chainCard("contract_deploy", sign, view);
    expect(c.kind).toBe("chain");
    if (c.kind !== "chain") return;
    const rows = Object.fromEntries(c.rows.map((r) => [r.k, r.v]));
    expect(rows).toMatchObject({ Action: "Deploy contract (412 bytes)", To: "contract creation", Cost: "≈0.002 SALT gas", Chain: "40204", Origin: "agent:hermes" });
    expect(c.raw).toBe(false);
    expect(c.summary).toMatch(/signature/i);
    expect(c.summary).toContain("Deploy contract");
  });

  it("Given calldata the decoder could not read, then the card is raw and never dresses it as decoded", () => {
    const c = chainCard("contract_deploy", sign, { ...view, decoded: { action: "Unrecognized", destination: "0xabc", cost: "" }, requiresRawAck: true });
    if (c.kind !== "chain") throw new Error("chain card expected");
    expect(c.raw).toBe(true);
    expect(c.rows.map((r) => r.k)).not.toContain("Action");
    expect(c.summary).toMatch(/could not be decoded/i);
  });

  it("Given no decoded cost, then the caller's fallback is shown instead of an empty cell", () => {
    const c = chainCard("contract_deploy", sign, { ...view, decoded: { ...view.decoded, cost: "" } }, "network gas");
    if (c.kind !== "chain") throw new Error("chain card expected");
    expect(c.rows.find((r) => r.k === "Cost")!.v).toBe("network gas");
  });
});

describe("Feature: command card shows the exact argv", () => {
  it("Given an argv array, then each argument is shown verbatim and separately (no shell joining)", () => {
    const c = commandCard("run", write, ["git", "commit", "-m", "two words; rm -rf ~"]);
    if (c.kind !== "command") throw new Error("command card expected");
    expect(c.argv).toEqual(["git", "commit", "-m", "two words; rm -rf ~"]);
    expect(c.summary).toContain("git");
    expect(c.summary).toMatch(/run a command/i);
  });

  it("Given tool arguments, then argv is taken only from a real string array, never split from a string", () => {
    expect(argvOf({ argv: ["ls", "-la"] })).toEqual(["ls", "-la"]);
    expect(argvOf({ argv: "ls -la" })).toBeNull();
    expect(argvOf({ argv: ["ls", 3] })).toBeNull();
    expect(argvOf({ argv: [] })).toBeNull();
    expect(argvOf({ command: "ls" })).toBeNull();
  });
});

describe("Feature: fields card and the plain-language summary", () => {
  it("Given a tool with arguments, then the card lists them by name and summarizes the effect in one line", () => {
    const c = fieldsCard("group_create", write, { name: "Book club", kind: "channel" }, "create the group “Book club”");
    if (c.kind !== "fields") throw new Error("fields card expected");
    expect(c.rows).toEqual([
      { k: "name", v: "Book club" },
      { k: "kind", v: "channel" },
    ]);
    expect(c.summary).toBe("Hermes wants to change something: create the group “Book club”");
  });

  it("Given each effect, then the summary verb says what kind of action it is", () => {
    expect(summaryLine({ effect: "none", trust: "trusted" }, "x")).toMatch(/^Hermes wants to read/);
    expect(summaryLine({ effect: "write", trust: "trusted" }, "x")).toMatch(/^Hermes wants to change something/);
    expect(summaryLine({ effect: "spend", trust: "trusted" }, "x")).toMatch(/^Hermes wants to move value/);
    expect(summaryLine({ effect: "sign", trust: "trusted" }, "x")).toMatch(/^Hermes wants your signature/);
    expect(summaryLine(null, "x")).toMatch(/^Hermes wants to act/);
  });

  it("Given an over-long value, then the field is truncated with its real length", () => {
    const c = fieldsCard("memory_assert", write, { fact: "z".repeat(900) }, "remember a fact");
    if (c.kind !== "fields") throw new Error("fields card expected");
    expect(c.rows[0].v.length).toBeLessThan(700);
    expect(c.rows[0].v).toContain("900 characters");
  });
});

describe("Feature: a malformed ceremony view", () => {
  it("Given a view with no decoded action, then the chain card is raw rather than empty-but-decoded", () => {
    const c = chainCard("contract_deploy", sign, { id: "x", origin: "o", kind: "transaction", chainId: 40204, requiresRawAck: false } as unknown as CeremonyView);
    expect(c.kind === "chain" && c.raw).toBe(true);
  });
});
