// =====================================================================
// HUP-S4.2 — citrate-node MCP server: webview types, the IO seam, and the
// connect-command builders shown in Settings.
//
// The server itself lives in Rust (src-tauri/src/node_mcp*.rs). This file only
// talks to its six commands. The connect token is shown once, right after it is
// created, and is never put in AppState or localStorage.
// =====================================================================

export interface NodeMcpToken {
  id: string;
  label: string;
  createdMs: number;
  lastUsedMs: number | null;
  /** Limited to the read tools (the token Citrate Core issues for Hermes). */
  readOnly?: boolean;
}

export interface NodeMcpCall {
  atMs: number;
  tokenId: string;
  method: string;
  tool: string | null;
  ok: boolean;
}

export interface NodeMcpStatus {
  running: boolean;
  enabled: boolean;
  port: number;
  endpoint: string;
  lastError: string | null;
  tokens: NodeMcpToken[];
  pendingRequests: number;
  recentCalls: NodeMcpCall[];
  stdioCommand: string | null;
}

export interface NodeMcpTokenIssued {
  id: string;
  label: string;
  createdMs: number;
  connectToken: string;
}

export interface CeremonyDecoded {
  action: string;
  cost: string;
  destination: string;
}

/** One write request (the Rust `McpRequest`, flattened kind + state). */
export interface NodeMcpRequest {
  id: string;
  tokenId: string;
  origin: string;
  summary: string;
  kind: "signature" | "action";
  ceremony_id?: string;
  ceremony?: { id: string; origin: string; decoded: CeremonyDecoded; requiresRawAck: boolean };
  action?: { action: string; [k: string]: unknown };
  state: "pending" | "running" | "approved" | "rejected" | "failed" | "expired";
  result?: unknown;
  reason?: string;
  error?: string;
  createdMs: number;
  decidedMs: number | null;
}

/** The IO seam: the runtime and the timeout-wrapped invoke. */
export interface NodeMcpIo {
  mode: "tauri" | "sim";
  invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T>;
}

export const DESKTOP_ONLY =
  "The node MCP server runs in the desktop app. This web preview has no node to serve, so there is nothing to connect to.";

/** The production IO. */
export async function desktopNodeMcpIo(): Promise<NodeMcpIo> {
  const { BRIDGE_MODE } = await import("../bridge/mode");
  return {
    mode: BRIDGE_MODE,
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      const { invoke } = await import("../bridge/tauri/invoke");
      return invoke<T>(cmd, args);
    },
  };
}

/** Quote one shell argument for a POSIX shell (single quotes, embedded quotes escaped). */
export function shellQuote(s: string): string {
  if (/^[A-Za-z0-9_./:@=-]+$/.test(s)) return s;
  return "'" + s.replace(/'/g, "'\\''") + "'";
}

/** `claude mcp add` over streamable HTTP (Claude Code sends the header on every request). */
export function claudeHttpCommand(endpoint: string, token: string): string {
  return `claude mcp add --transport http citrate-node ${shellQuote(endpoint)} --header ${shellQuote("Authorization: Bearer " + token)}`;
}

/** `claude mcp add` through the stdio shim (the app binary with `--mcp-stdio`). */
export function claudeStdioCommand(exe: string, token: string, port: number): string {
  const env = [`CITRATE_NODE_MCP_TOKEN=${token}`];
  if (port !== 47204) env.push(`CITRATE_NODE_MCP_PORT=${port}`);
  const envFlags = env.map((e) => `-e ${shellQuote(e)}`).join(" ");
  return `claude mcp add citrate-node ${envFlags} -- ${shellQuote(exe)} --mcp-stdio`;
}

/** Human label for a request state. */
export function stateLabel(r: NodeMcpRequest): string {
  switch (r.state) {
    case "pending":
      return "waiting for you";
    case "running":
      return "running";
    case "approved":
      return "approved";
    case "rejected":
      return r.reason ? `rejected (${r.reason})` : "rejected";
    case "failed":
      return r.error ? `failed: ${r.error}` : "failed";
    case "expired":
      return "expired before a decision";
  }
}

/** A short time-ago string for a Unix-ms timestamp. */
export function ago(ms: number | null, now: number): string {
  if (!ms) return "never";
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  return `${Math.round(m / 60)}h ago`;
}
