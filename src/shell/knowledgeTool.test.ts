// HUP-S3.1: the in-app memory_search tool answers knowledge questions from the bundled corpus with
// passages and citations, and keeps personal notes title-only.
import { describe, it, expect, vi, afterEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import type { ToolCall } from "../agent/harness";

const call = (args: Record<string, unknown>): ToolCall => ({ id: "c1", name: "memory_search", arguments: JSON.stringify(args) });
const noop = () => {};

afterEach(() => vi.restoreAllMocks());

describe("Feature: Hermes searches the bundled knowledge with citations", () => {
  it("Given a knowledge tenant, then the search asks for passages and the model gets the citation to quote", async () => {
    const search = vi.spyOn(bridge.memory, "search").mockResolvedValue({
      tenant: "refs",
      totalInTenant: 1117,
      hits: [
        {
          id: "0a1b2c3d4e",
          kind: "doc",
          title: "ERC20 › Supply",
          cite: "openzeppelin-contracts:docs/modules/ROOT/pages/erc20.adoc#supply",
          passage: "ERC20 › Supply\n\nThe total supply is fixed at deployment.",
        },
      ],
    });
    const out = await store.handleTool(call({ query: "erc20 supply", tenant: "refs" }), "m1", noop);
    expect(search).toHaveBeenCalledWith("refs", "erc20 supply", 5, { passages: true });
    expect(out).toContain("cite as openzeppelin-contracts:docs/modules/ROOT/pages/erc20.adoc#supply");
    expect(out).toContain("The total supply is fixed at deployment.");
  });

  it("Given no tenant, then the Citrate docs are searched with passages", async () => {
    const search = vi.spyOn(bridge.memory, "search").mockResolvedValue({ tenant: "citrate-docs", totalInTenant: 0, hits: [] });
    const out = await store.handleTool(call({ query: "chain id" }), "m1", noop);
    expect(search).toHaveBeenCalledWith("citrate-docs", "chain id", 5, { passages: true });
    expect(out).toMatch(/No results in the citrate-docs memory/);
  });

  it("Given the personal tenant, then the search stays title-only", async () => {
    const search = vi.spyOn(bridge.memory, "search").mockResolvedValue({
      tenant: "personal",
      totalInTenant: 3,
      hits: [{ id: "a", kind: "Rationale", title: "prefers reduced telemetry" }],
    });
    const out = await store.handleTool(call({ query: "telemetry", tenant: "personal" }), "m1", noop);
    expect(search).toHaveBeenCalledWith("personal", "telemetry", 6, undefined);
    expect(out).toContain("[1] prefers reduced telemetry");
  });
});
