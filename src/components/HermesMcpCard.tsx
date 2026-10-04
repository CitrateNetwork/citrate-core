// =====================================================================
// HUP-S4.3 — "Connected tools (MCP)": which MCP servers Hermes may use.
//
// The member's local memory graph (mem-mcp through core's read-only bridge), the CitrateScan
// explorer (read-only), and (HUP-S4.1 / S4.2 / S8.5) this node's own MCP server, offered to Hermes
// with read tools only while its write switch is off (pending owner sign-off). Core writes the
// sidecar's allowlist file from these switches; the sidecar reads it when Hermes starts. All are
// OFF by default (the default is pending owner sign-off). HUP-S4.1: each row also shows the
// server's live state from the running sidecar. Presentational: the container passes the view,
// the runtime view and a toggle handler.
// =====================================================================
import { useEffect, useState } from "react";
import type { HermesMcpSettings, HermesMcpView, McpRuntimeView } from "../bridge/domains";
import { bridge } from "../bridge";
import { runtimeLine, type RuntimeTone } from "../surfaces/mcpRuntime";

const RUNTIME_TONE: Record<RuntimeTone, string> = {
  ok: "var(--ok, var(--tx-2))",
  warn: "var(--warn)",
  danger: "var(--danger)",
  muted: "var(--tx-3)",
};

export interface HermesMcpCardProps {
  view: HermesMcpView | null;
  error?: string | null;
  onToggle: (next: HermesMcpSettings) => void;
  /** HUP-S4.1: the running sidecar's servers (null while loading or unavailable). */
  runtime?: McpRuntimeView | null;
}

export function HermesMcpCard({ view, error, onToggle, runtime = null }: HermesMcpCardProps) {
  return (
    <div className="surface" style={{ display: "flex", flexDirection: "column" }} data-testid="hermes-mcp-card">
      <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
        <span style={{ fontSize: 13.5, fontWeight: 500 }}>Connected tools (MCP)</span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
          writes need you
        </span>
      </div>
      {!view ? (
        <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: "18px 20px" }}>
          {error || "Loading connected tools."}
        </p>
      ) : (
        <>
          {view.servers.map((sv) => {
            const key = sv.name as keyof HermesMcpSettings;
            const on = Boolean(view.settings[key]);
            return (
              <div key={sv.name} style={{ display: "flex", alignItems: "flex-start", gap: 12, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
                <span style={{ flex: 1, minWidth: 0 }}>
                  <span style={{ display: "block", fontSize: 12.5, fontWeight: 500 }}>{sv.label}</span>
                  <span style={{ display: "block", fontSize: 12, color: sv.available ? "var(--tx-2)" : "var(--warn)", marginTop: 3, lineHeight: 1.5 }}>{sv.detail}</span>
                  {on &&
                    (() => {
                      const line = runtimeLine(runtime, sv.name);
                      return line ? (
                        <span data-testid={`mcp-runtime-${sv.name}`} className="mono" style={{ display: "block", fontSize: 10.5, color: RUNTIME_TONE[line.tone], marginTop: 4 }}>
                          {line.text}
                        </span>
                      ) : null;
                    })()}
                </span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={on}
                  aria-label={sv.label}
                  disabled={!sv.available}
                  className={on ? "btn btn-primary btn-sm" : "btn btn-sm"}
                  onClick={() => onToggle({ ...view.settings, [key]: !on })}
                >
                  {on ? "On" : "Off"}
                </button>
              </div>
            );
          })}
          {error && (
            <p style={{ fontSize: 12, lineHeight: 1.5, color: "var(--warn)", margin: 0, padding: "10px 16px 0" }}>{error}</p>
          )}
          <p style={{ fontSize: 11.5, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: "10px 16px" }}>
            {view.restartRequired
              ? "Saved. Restart Hermes to apply: it reads these at start."
              : "Applied the next time Hermes starts."}{" "}
            Results from these tools are treated as untrusted, so after one is used Hermes asks you before any action that changes something. All start off (default pending owner sign-off).
          </p>
        </>
      )}
    </div>
  );
}

/** The card wired to the bridge: loads the view, saves a toggle, reports failures in place. */
export function HermesMcpPanel() {
  const [view, setView] = useState<HermesMcpView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [runtime, setRuntime] = useState<McpRuntimeView | null>(null);
  useEffect(() => {
    let live = true;
    bridge.agentHarness
      .mcpSettings()
      .then((v) => live && setView(v))
      .catch((e: unknown) => live && setError(e instanceof Error ? e.message : String(e)));
    // HUP-S4.1: the live state is informative only; a failure to read it leaves the rows as they are.
    bridge.agentHarness
      .mcpRuntime()
      .then((r) => live && setRuntime(r))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, []);
  const onToggle = (next: HermesMcpSettings) => {
    bridge.agentHarness
      .mcpSet(next)
      .then((v) => {
        setView(v);
        setError(null);
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  };
  return <HermesMcpCard view={view} error={error} onToggle={onToggle} runtime={runtime} />;
}
