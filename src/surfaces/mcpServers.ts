// =====================================================================
// HUP-S4.4 — Settings > MCP servers (webview half).
//
// A member adds their own MCP servers. Rust (src-tauri/src/mcp_servers.rs)
// validates and stores each entry DISABLED; the member then checks it, which
// runs a dry-run probe inside the Hermes sidecar (start, initialize, list
// tools, stop; nothing is registered) and shows a review screen. Only a
// successful check of the entry exactly as it stands can enable it, and only
// enabled entries are written to the allowlist the sidecar reads at start.
//
// Env values never come back to the webview: Rust returns masks, and an edit
// that does not retype a value sends `null` ("keep the stored value").
// Every MCP tool is untrusted; write tools are off unless turned on per server.
// =====================================================================

export type McpTransport = "stdio" | "http";

export interface McpEnvView {
  key: string;
  masked: string;
}

export interface McpServerView {
  name: string;
  transport: McpTransport;
  command: string | null;
  args: string[];
  cwd: string | null;
  url: string | null;
  env: McpEnvView[];
  allowWriteTools: boolean;
  enabled: boolean;
  needsReview: boolean;
}

export interface McpFieldError {
  field: string;
  message: string;
}

export interface McpProbeTool {
  name: string;
  exposedName: string | null;
  description: string;
  annotations: {
    readOnlyHint: boolean | null;
    destructiveHint: boolean | null;
    idempotentHint: boolean | null;
    openWorldHint: boolean | null;
    title: string | null;
  };
  effective: { readOnly: boolean; destructive: boolean; idempotent: boolean; openWorld: boolean };
  trust: string;
  offered: boolean;
  skipReason: string | null;
}

export interface McpProbeReport {
  name: string;
  transport: string;
  ok: boolean;
  error: string | null;
  protocolVersion: string | null;
  serverName: string | null;
  serverVersion: string | null;
  capabilities: unknown;
  tools: McpProbeTool[];
  toolsTruncated: boolean;
  allowWriteTools: boolean;
}

export interface McpReview {
  server: McpServerView;
  commandLine: string;
  probe: McpProbeReport;
  reviewToken: string;
  canEnable: boolean;
}

export interface McpReviewResult {
  errors: McpFieldError[];
  review: McpReview | null;
}

export interface McpSaveResult {
  ok: boolean;
  errors: McpFieldError[];
  servers: McpServerView[];
}

/** The save command's input (camelCase; Rust `ServerInput`). */
export interface McpServerInput {
  name: string;
  transport: McpTransport;
  command: string | null;
  args: string[];
  cwd: string | null;
  url: string | null;
  env: { key: string; value: string | null }[];
  allowWriteTools: boolean;
  previousName: string | null;
}

/** One env row in the form. `keep` = a stored value exists and an empty field keeps it. */
export interface McpEnvDraft {
  key: string;
  value: string;
  keep: boolean;
}

export interface McpDraft {
  name: string;
  transport: McpTransport;
  command: string;
  argsText: string;
  cwd: string;
  url: string;
  env: McpEnvDraft[];
  allowWriteTools: boolean;
}

/** The IO seam: the runtime mode and the timeout-wrapped invoke. */
export interface McpIo {
  mode: "tauri" | "sim";
  invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T>;
}

export function emptyDraft(): McpDraft {
  return { name: "", transport: "stdio", command: "", argsText: "", cwd: "", url: "", env: [], allowWriteTools: false };
}

export function draftFromServer(s: McpServerView): McpDraft {
  return {
    name: s.name,
    transport: s.transport,
    command: s.command ?? "",
    argsText: s.args.join("\n"),
    cwd: s.cwd ?? "",
    url: s.url ?? "",
    env: s.env.map((e) => ({ key: e.key, value: "", keep: true })),
    allowWriteTools: s.allowWriteTools,
  };
}

/** The form as the save command's input. Arguments are one per line (blank lines dropped). */
export function draftToInput(d: McpDraft, previousName: string | null): McpServerInput {
  const stdio = d.transport === "stdio";
  return {
    name: d.name.trim(),
    transport: d.transport,
    command: stdio ? d.command.trim() : null,
    args: stdio ? d.argsText.split("\n").map((a) => a.trim()).filter((a) => a.length > 0) : [],
    cwd: stdio && d.cwd.trim() ? d.cwd.trim() : null,
    url: stdio ? null : d.url.trim(),
    env: stdio ? d.env.filter((e) => e.key.trim()).map((e) => ({ key: e.key.trim(), value: e.keep && e.value === "" ? null : e.value })) : [],
    allowWriteTools: d.allowWriteTools,
    previousName,
  };
}

export function errorFor(errors: McpFieldError[], field: string): string | null {
  return errors.find((e) => e.field === field)?.message ?? null;
}

export type BadgeTone = "ok" | "warn" | "danger" | "muted";

/** The annotation badges for one tool, always ending in "untrusted". */
export function toolBadges(t: McpProbeTool): { label: string; tone: BadgeTone }[] {
  const out: { label: string; tone: BadgeTone }[] = [];
  out.push(t.effective.readOnly ? { label: "read-only", tone: "ok" } : { label: "writes", tone: "warn" });
  if (t.effective.destructive) out.push({ label: "destructive", tone: "danger" });
  if (t.effective.idempotent) out.push({ label: "idempotent", tone: "muted" });
  if (t.effective.openWorld) out.push({ label: "open world", tone: "warn" });
  out.push({ label: "untrusted", tone: "warn" });
  return out;
}

/** The production IO: the bridge's runtime mode and the timeout-wrapped invoke. */
export async function desktopMcpIo(): Promise<McpIo> {
  const { BRIDGE_MODE } = await import("../bridge/mode");
  return {
    mode: BRIDGE_MODE,
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      const { invoke } = await import("../bridge/tauri/invoke");
      return invoke<T>(cmd, args);
    },
  };
}
