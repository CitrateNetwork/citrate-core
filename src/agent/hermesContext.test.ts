// FL design board 01: "What Hermes can see" is read from the tools the chat's session was given.
import { describe, it, expect } from "vitest";
import { contextRows, contextSummary } from "./hermesContext";
import { annotatedAgentTools } from "./toolAnnotations";

const ALL = annotatedAgentTools().map((t) => t.function.name);

describe("contextRows", () => {
  it("the sidecar loop with core's tools sees node, network, notes, groups and rounds; never keys", () => {
    const rows = contextRows({ toolNames: ALL, providerKind: "sidecar", hermesDown: false, modelLabel: "Local 8B" });
    const can = Object.fromEntries(rows.map((r) => [r.id, r.can]));
    expect(can).toMatchObject({ node: true, chain: true, memory: true, groups: true, rounds: true, folders: true, datasets: false, keys: false });
    expect(rows.find((r) => r.id === "groups")?.detail).toBe("names and rosters, not their messages");
    expect(rows.find((r) => r.id === "keys")?.detail).toContain("never");
  });

  it("the no-tools fallback sees nothing, and says why", () => {
    const i = { toolNames: [], providerKind: "local", hermesDown: true, modelLabel: "Local 8B" };
    expect(contextRows(i).filter((r) => r.can)).toEqual([]);
    expect(contextSummary(i)).toContain("with no tools");
  });

  it("a session without a group of tools says it has no tool for it", () => {
    const rows = contextRows({ toolNames: ["node_status"], providerKind: "agent", hermesDown: false, modelLabel: "Gateway" });
    expect(rows.find((r) => r.id === "node")?.can).toBe(true);
    expect(rows.find((r) => r.id === "chain")?.detail).toBe("no tool for it in this chat");
    // Folders and commands are the sidecar loop's own tools.
    expect(rows.find((r) => r.id === "folders")?.can).toBe(false);
    expect(contextSummary({ toolNames: ["node_status"], providerKind: "agent", hermesDown: false, modelLabel: "Gateway" })).toBe("Gateway answers, with 1 core tool.");
  });
});
