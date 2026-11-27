// L6-WP01 (AR-E8.2, D8): whether a connection is available to the member's agent.
//
// Data source (Rule 7): the MCP gateway status report, `mcp_gateway_status` (L1 AR-E1.10, interface
// in lanes/L6 section 3.6). A connector counts as an agent tool only when that report lists a live
// server for it with at least one tool. This build has no MCP gateway, so the report is the real
// state of the build: not running, no servers. Every connected row therefore says it is not yet
// available to the agent. When L1 ships the gateway, `gatewayReport()` reads it (WP11 / WP13).

/** One server in the gateway report (frozen shape from L1's `mcp_gateway_status`). */
export interface GatewayServer {
  name: string;
  connectorId?: string;
  live: boolean;
  toolCount: number;
  error: string | null;
}

export interface GatewayReport {
  running: boolean;
  /** Why the gateway is not running, in member words; null while it runs. */
  reason: string | null;
  servers: GatewayServer[];
}

export const GATEWAY_NOT_IN_BUILD = "The MCP gateway is not part of this build, so connections are not yet agent tools.";

/** The gateway status of this build. */
export function gatewayReport(): GatewayReport {
  return { running: false, reason: GATEWAY_NOT_IN_BUILD, servers: [] };
}

export interface AgentToolState {
  live: boolean;
  reason: string | null;
}

/** Whether the connector `service` is an agent tool according to `report`. */
export function agentToolsFor(report: GatewayReport, service: string): AgentToolState {
  if (!report.running) return { live: false, reason: report.reason };
  const server = report.servers.find((s) => s.connectorId === service);
  if (!server) return { live: false, reason: "The MCP gateway reports no server for this connection." };
  if (!server.live || server.toolCount < 1) return { live: false, reason: server.error ?? "The MCP gateway reports no tools for this connection." };
  return { live: true, reason: null };
}

/** The label a connected row carries. */
export function connectedLabel(state: AgentToolState): string {
  return state.live ? "Connected. Available to your agent" : "Connected. Not yet available to your agent";
}
