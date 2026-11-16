// #690: what the member attaches with @, and what Hermes is told about it.
import { describe, it, expect } from "vitest";
import { contextPreamble, datasetAttachment, groupAttachment, journalAttachment, matchAttachments, mentionAt, MAX_JOURNAL_CHARS } from "./chatAttachments";

const DS = { name: "1794800000000.jsonl", builtMs: 1794800000000, bytes: 1_250_000, examples: 500, sha256: "ab".repeat(32) };

describe("attachments", () => {
  it("a dataset is described by name, size, count and hash; its lines are never included", () => {
    const a = datasetAttachment(DS);
    expect(a.label).toBe("Dataset · 500 conversations, 2026-11-16");
    expect(a.context).toContain("sha256 " + "ab".repeat(32));
    expect(a.context).toContain("1.3 MB");
    expect(a.context).toContain("Its lines are not included");
  });

  it("a group is described without its messages; a journal page carries the member's own text, clipped", () => {
    const g = groupAttachment({ id: "g1", name: "Orchard Lab", owner: "0x1", kind: "channel", members: [{} as never, {} as never] });
    expect(g.context).toBe('Group "Orchard Lab" (id g1, a channel, 2 members). Its messages are not included.');
    const long = "x".repeat(MAX_JOURNAL_CHARS + 50);
    const j = journalAttachment({ id: "p1", title: "Node log", kind: "page", pinned: false, blocks: [long] });
    expect(j.context.length).toBeLessThan(MAX_JOURNAL_CHARS + 80);
    expect(j.context.endsWith("…")).toBe(true);
  });

  it("@ starts a word; the query is what follows it", () => {
    expect(mentionAt("look at @orch")).toEqual({ start: 8, query: "orch" });
    expect(mentionAt("@")).toEqual({ start: 0, query: "" });
    expect(mentionAt("mail a@b")).toBeNull();
    expect(mentionAt("done @x then")).toBeNull();
  });

  it("matches by label", () => {
    const all = [datasetAttachment(DS), groupAttachment({ id: "g1", name: "Orchard Lab", owner: "", kind: "channel", members: [] })];
    expect(matchAttachments(all, "orch").map((a) => a.id)).toEqual(["group:g1"]);
    expect(matchAttachments(all, "")).toHaveLength(2);
  });

  it("the preamble lists attachments and what is off; nothing when neither", () => {
    expect(contextPreamble([], [])).toBe("");
    const p = contextPreamble([datasetAttachment(DS)], ["Your groups"]);
    expect(p).toContain("The member attached this for context");
    expect(p).toContain("The member turned off for this chat: Your groups.");
    expect(p.endsWith("The member's message:\n")).toBe(true);
  });
});
