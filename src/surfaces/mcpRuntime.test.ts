// HUP-S4.1: the live MCP server line shown in the settings cards.
import { describe, it, expect } from "vitest";
import { runtimeLine } from "./mcpRuntime";
import type { McpRuntimeView } from "../bridge/domains";

const RT: McpRuntimeView = {
  running: true,
  configured: true,
  servers: [
    { name: "node", transport: "stdio", state: "ready", era: "legacy", protocolVersion: "2025-06-18", tools: 21, skipped: [] },
    { name: "scan", transport: "http", state: "ready", era: "modern", protocolVersion: "2026-07-28", tasks: true, tools: 1, skipped: [] },
    { name: "notes", transport: "stdio", state: "failed", tools: 0, skipped: [], error: "could not start the server: no such file", nextRetryMs: 3400 },
    { name: "mem", transport: "stdio", state: "exited", tools: 0, skipped: [], nextRetryMs: 0 },
  ],
};

describe("runtimeLine", () => {
  it("is null until the runtime view loads", () => {
    expect(runtimeLine(null, "node")).toBeNull();
  });
  it("says when Hermes is not running or has not loaded the server", () => {
    expect(runtimeLine({ running: false }, "node")).toEqual({ text: "Hermes is not running", tone: "muted" });
    expect(runtimeLine(RT, "other")?.text).toContain("restart Hermes");
  });
  it("shows connected servers with their tool count and protocol", () => {
    expect(runtimeLine(RT, "node")).toEqual({ text: "connected · 21 tools · MCP 2025-06-18", tone: "ok" });
    expect(runtimeLine(RT, "scan")).toEqual({ text: "connected · 1 tool · MCP 2026-07-28 · tasks", tone: "ok" });
  });
  it("shows failures with the reason and the next retry", () => {
    expect(runtimeLine(RT, "notes")).toEqual({ text: "failed: could not start the server: no such file · retrying in 4 s", tone: "danger" });
    expect(runtimeLine(RT, "mem")).toEqual({ text: "reconnecting · retrying now", tone: "warn" });
  });
});
