// =====================================================================
// HUP-S4.1: the live state of an MCP server as the running Hermes sidecar reports it
// (`GET /mcp/servers` through core's `mcp_servers_runtime`): connected with N tools, failed (and
// when it is retried), or reconnecting. One line per server for the settings cards.
// =====================================================================
import type { McpRuntimeView } from "../bridge/domains";

export type RuntimeTone = "ok" | "warn" | "danger" | "muted";

export interface RuntimeLine {
  text: string;
  tone: RuntimeTone;
}

function retry(ms: number | undefined): string {
  if (ms === undefined) return "";
  const s = Math.max(0, Math.ceil(ms / 1000));
  return s === 0 ? " · retrying now" : ` · retrying in ${s} s`;
}

function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

/** The status line for server `name`, or null while the runtime view has not loaded. */
export function runtimeLine(rt: McpRuntimeView | null, name: string): RuntimeLine | null {
  if (!rt) return null;
  if (!rt.running) return { text: "Hermes is not running", tone: "muted" };
  const s = (rt.servers ?? []).find((x) => x.name === name);
  if (!s) return { text: "not loaded by the running Hermes (restart Hermes to apply)", tone: "muted" };
  if (s.state === "ready") {
    const extras = [s.era === "modern" ? "MCP 2026-07-28" : s.protocolVersion ? `MCP ${s.protocolVersion}` : "", s.tasks ? "tasks" : ""].filter(Boolean);
    return { text: ["connected", plural(s.tools, "tool"), ...extras].join(" · "), tone: "ok" };
  }
  if (s.state === "exited") return { text: `reconnecting${retry(s.nextRetryMs)}`, tone: "warn" };
  const why = s.error ? `: ${s.error.length > 160 ? s.error.slice(0, 160) + "…" : s.error}` : "";
  return { text: `failed${why}${retry(s.nextRetryMs)}`, tone: "danger" };
}
