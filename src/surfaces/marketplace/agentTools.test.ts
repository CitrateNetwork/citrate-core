// L6-WP01: a connection is called an agent tool only when the MCP gateway report says it is live
// with at least one tool. Every other state, including this build's "no gateway", is not.
import { describe, expect, it } from "vitest";
import { agentToolsFor, connectedLabel, gatewayReport, GATEWAY_NOT_IN_BUILD, type GatewayReport } from "./agentTools";

const running = (servers: GatewayReport["servers"]): GatewayReport => ({ running: true, reason: null, servers });

describe("L6-WP01 agent tool state comes only from the gateway report", () => {
  it("this_build_has_no_gateway_so_nothing_is_an_agent_tool", () => {
    const report = gatewayReport();
    expect(report.running).toBe(false);
    expect(report.servers).toEqual([]);
    for (const svc of ["github", "gdrive", "notion", "gsheets", "gcal"]) {
      const state = agentToolsFor(report, svc);
      expect(state).toEqual({ live: false, reason: GATEWAY_NOT_IN_BUILD });
      expect(connectedLabel(state)).toBe("Connected. Not yet available to your agent");
    }
  });

  it("a_running_gateway_without_a_server_for_the_connector_is_not_an_agent_tool", () => {
    const state = agentToolsFor(running([{ name: "notion", connectorId: "notion", live: true, toolCount: 4, error: null }]), "github");
    expect(state.live).toBe(false);
    expect(state.reason).toMatch(/no server/);
  });

  it("a_server_that_is_down_or_has_no_tools_is_not_an_agent_tool", () => {
    const down = agentToolsFor(running([{ name: "github", connectorId: "github", live: false, toolCount: 6, error: "upstream 401" }]), "github");
    expect(down).toEqual({ live: false, reason: "upstream 401" });
    const empty = agentToolsFor(running([{ name: "github", connectorId: "github", live: true, toolCount: 0, error: null }]), "github");
    expect(empty.live).toBe(false);
    expect(connectedLabel(empty)).toBe("Connected. Not yet available to your agent");
  });

  it("a_server_matched_by_name_only_is_not_trusted_as_the_connector", () => {
    // The connector id is the link; a server that merely shares the service's name does not count.
    const state = agentToolsFor(running([{ name: "github", live: true, toolCount: 6, error: null }]), "github");
    expect(state.live).toBe(false);
  });

  it("a_live_server_with_tools_for_the_connector_is_an_agent_tool", () => {
    const state = agentToolsFor(running([{ name: "github", connectorId: "github", live: true, toolCount: 6, error: null }]), "github");
    expect(state).toEqual({ live: true, reason: null });
    expect(connectedLabel(state)).toBe("Connected. Available to your agent");
  });
});
