// =====================================================================
// HUP-S4.2 + S8.5 — Settings panel for the citrate-node MCP server.
//
// Turn the loopback server on or off, create and revoke connect tokens, copy
// the ready-made Claude Code command, and approve or reject what connected
// agents ask for. Every write an agent makes waits here: transactions run
// through the signature ceremony only when approved, and cluster / invite
// changes run only when approved.
// =====================================================================
import { useCallback, useEffect, useState } from "react";
import {
  DESKTOP_ONLY,
  ago,
  claudeHttpCommand,
  claudeStdioCommand,
  stateLabel,
  type NodeMcpIo,
  type NodeMcpRequest,
  type NodeMcpStatus,
  type NodeMcpTokenIssued,
} from "./nodeMcp";

export interface NodeMcpPanelProps {
  io: () => Promise<NodeMcpIo>;
  now?: () => number;
  /** Poll interval (ms) while the panel is open; 0 disables polling (tests). */
  pollMs?: number;
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

const mono = { fontFamily: "var(--font-mono, monospace)", fontSize: 11.5 } as const;

export function NodeMcpPanel({ io, now = () => Date.now(), pollMs = 2500 }: NodeMcpPanelProps) {
  const [x, setX] = useState<NodeMcpIo | null>(null);
  const [status, setStatus] = useState<NodeMcpStatus | null>(null);
  const [requests, setRequests] = useState<NodeMcpRequest[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [label, setLabel] = useState("Claude Code");
  const [issued, setIssued] = useState<NodeMcpTokenIssued | null>(null);
  const [rawAck, setRawAck] = useState<Record<string, boolean>>({});

  const refresh = useCallback(async (inst: NodeMcpIo) => {
    if (inst.mode !== "tauri") return;
    try {
      const [s, r] = await Promise.all([
        inst.invoke<NodeMcpStatus>("node_mcp_status", {}),
        inst.invoke<NodeMcpRequest[]>("node_mcp_requests", {}),
      ]);
      setStatus(s);
      setRequests(r);
    } catch (e) {
      setError(message(e));
    }
  }, []);

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setInterval> | null = null;
    void io().then((inst) => {
      if (!live) return;
      setX(inst);
      void refresh(inst);
      if (inst.mode === "tauri" && pollMs > 0) timer = setInterval(() => void refresh(inst), pollMs);
    });
    return () => {
      live = false;
      if (timer) clearInterval(timer);
    };
  }, [io, pollMs, refresh]);

  if (x && x.mode !== "tauri") {
    return (
      <div className="surface" data-testid="nodemcp-panel" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 6 }}>
        <span className="eyebrow">Node MCP server</span>
        <span data-testid="nodemcp-desktop-only" style={{ fontSize: 12.5, color: "var(--tx-3)" }}>
          {DESKTOP_ONLY}
        </span>
      </div>
    );
  }

  const run = async (key: string, f: (inst: NodeMcpIo) => Promise<void>) => {
    if (!x || busy) return;
    setBusy(key);
    setError(null);
    try {
      await f(x);
      await refresh(x);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(null);
    }
  };

  const toggle = () =>
    run("toggle", async (inst) => {
      setStatus(await inst.invoke<NodeMcpStatus>("node_mcp_set_enabled", { enabled: !status?.running }));
    });
  const create = () =>
    run("create", async (inst) => {
      setIssued(await inst.invoke<NodeMcpTokenIssued>("node_mcp_token_create", { label }));
    });
  const revoke = (id: string) =>
    run("revoke:" + id, async (inst) => {
      await inst.invoke<boolean>("node_mcp_token_revoke", { id });
      if (issued?.id === id) setIssued(null);
    });
  const decide = (r: NodeMcpRequest, approve: boolean) =>
    run("decide:" + r.id, async (inst) => {
      await inst.invoke<NodeMcpRequest>("node_mcp_decide", { id: r.id, approve, rawAck: !!rawAck[r.id] });
    });
  const copy = (text: string) => {
    void navigator.clipboard?.writeText(text).catch(() => {});
  };

  const pending = requests.filter((r) => r.state === "pending");
  const decided = requests.filter((r) => r.state !== "pending").slice(0, 8);
  const t = now();

  return (
    <div className="surface" data-testid="nodemcp-panel" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
        <span className="eyebrow">Node MCP server · let Claude Code or another agent use this node</span>
        <span style={{ fontSize: 12, color: "var(--tx-3)" }}>
          Off by default. When on, it listens on this computer only. Every request needs a connect token. Agents can read; anything that changes something
          waits for your approval below, and transactions are signed only through the normal signing ceremony.
        </span>
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
        <span data-testid="nodemcp-state" style={{ fontSize: 12.5, color: status?.running ? "var(--tx-1)" : "var(--tx-3)" }}>
          {status ? (status.running ? `Listening on ${status.endpoint}` : "Off") : "Loading…"}
        </span>
        <button className="btn btn-sm btn-secondary" data-testid="nodemcp-toggle" disabled={!status || busy !== null} onClick={() => void toggle()}>
          {status?.running ? "Turn off" : "Turn on"}
        </button>
      </div>
      {status?.lastError && (
        <span data-testid="nodemcp-last-error" style={{ fontSize: 12, color: "var(--danger)" }}>
          {status.lastError}
        </span>
      )}

      {/* ---------- requests awaiting the member ---------- */}
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <span className="eyebrow">Requests waiting for you ({pending.length})</span>
        {pending.length === 0 && <span style={{ fontSize: 12, color: "var(--tx-3)" }}>Nothing is waiting.</span>}
        {pending.map((r) => (
          <div key={r.id} data-testid={`nodemcp-req-${r.id}`} style={{ border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: 10, display: "flex", flexDirection: "column", gap: 6 }}>
            <span style={{ fontSize: 12.5, color: "var(--tx-1)" }}>{r.summary}</span>
            <span style={{ ...mono, color: "var(--tx-3)" }}>from {r.origin}</span>
            {r.kind === "signature" && r.ceremony && (
              <div style={{ ...mono, color: "var(--tx-2)", display: "flex", flexDirection: "column", gap: 2 }}>
                <span>action: {r.ceremony.decoded.action}</span>
                <span>cost: {r.ceremony.decoded.cost || "none"}</span>
                <span>to: {r.ceremony.decoded.destination || "unknown"}</span>
              </div>
            )}
            {r.kind === "signature" && r.ceremony?.requiresRawAck && (
              <label style={{ fontSize: 12, color: "var(--warn)", display: "flex", gap: 6, alignItems: "center" }}>
                <input
                  type="checkbox"
                  data-testid={`nodemcp-rawack-${r.id}`}
                  checked={!!rawAck[r.id]}
                  onChange={(e) => setRawAck({ ...rawAck, [r.id]: e.target.checked })}
                />
                This transaction's data could not be decoded. I have checked the raw data and still want to sign it.
              </label>
            )}
            <div style={{ display: "flex", gap: 8 }}>
              <button
                className="btn btn-sm btn-secondary"
                data-testid={`nodemcp-approve-${r.id}`}
                disabled={busy !== null || (r.kind === "signature" && !!r.ceremony?.requiresRawAck && !rawAck[r.id])}
                onClick={() => void decide(r, true)}
              >
                {r.kind === "signature" ? "Approve and sign" : "Approve"}
              </button>
              <button className="btn btn-sm btn-ghost" data-testid={`nodemcp-reject-${r.id}`} disabled={busy !== null} onClick={() => void decide(r, false)}>
                Reject
              </button>
            </div>
          </div>
        ))}
        {decided.length > 0 && (
          <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
            {decided.map((r) => (
              <span key={r.id} style={{ fontSize: 11.5, color: "var(--tx-3)" }}>
                {r.summary}: {stateLabel(r)}
              </span>
            ))}
          </div>
        )}
      </div>

      {/* ---------- connect tokens ---------- */}
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <span className="eyebrow">Connect tokens</span>
        <div style={{ display: "flex", gap: 8 }}>
          <input
            className="input"
            data-testid="nodemcp-label"
            value={label}
            maxLength={48}
            onChange={(e) => setLabel(e.target.value)}
            placeholder="Which client is this for?"
            style={{ flex: 1 }}
          />
          <button className="btn btn-sm btn-secondary" data-testid="nodemcp-create" disabled={busy !== null || !label.trim()} onClick={() => void create()}>
            Create token
          </button>
        </div>
        {issued && status && (
          <div data-testid="nodemcp-issued" style={{ border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: 10, display: "flex", flexDirection: "column", gap: 6 }}>
            <span style={{ fontSize: 12, color: "var(--warn)" }}>
              Copy this token now. It is shown once and is not stored; only a fingerprint of it is kept. Anyone with it can ask this node for things,
              though every change still needs your approval.
            </span>
            <code data-testid="nodemcp-token" style={{ ...mono, wordBreak: "break-all" }}>
              {issued.connectToken}
            </code>
            <span style={{ fontSize: 12, color: "var(--tx-2)" }}>Claude Code, over HTTP:</span>
            <code data-testid="nodemcp-cmd-http" style={{ ...mono, wordBreak: "break-all" }}>
              {claudeHttpCommand(status.endpoint, issued.connectToken)}
            </code>
            {status.stdioCommand && (
              <>
                <span style={{ fontSize: 12, color: "var(--tx-2)" }}>Or through the stdio shim (for clients that only speak stdio):</span>
                <code data-testid="nodemcp-cmd-stdio" style={{ ...mono, wordBreak: "break-all" }}>
                  {claudeStdioCommand(status.stdioCommand, issued.connectToken, status.port)}
                </code>
              </>
            )}
            <div style={{ display: "flex", gap: 8 }}>
              <button className="btn btn-sm btn-ghost" onClick={() => copy(issued.connectToken)}>
                Copy token
              </button>
              <button className="btn btn-sm btn-ghost" onClick={() => copy(claudeHttpCommand(status.endpoint, issued.connectToken))}>
                Copy Claude Code command
              </button>
              <button className="btn btn-sm btn-ghost" data-testid="nodemcp-issued-done" onClick={() => setIssued(null)}>
                Done
              </button>
            </div>
          </div>
        )}
        {status && status.tokens.length === 0 && <span style={{ fontSize: 12, color: "var(--tx-3)" }}>No tokens yet. No client can connect.</span>}
        {status?.tokens.map((tk) => (
          <div key={tk.id} data-testid={`nodemcp-token-${tk.id}`} style={{ display: "flex", alignItems: "center", gap: 10, borderBottom: "1px solid var(--line-1)", paddingBottom: 6 }}>
            <span style={{ fontSize: 12.5, color: "var(--tx-1)", flex: 1 }}>{tk.label}</span>
            {tk.readOnly && (
              <span data-testid={`nodemcp-readonly-${tk.id}`} style={{ fontSize: 11.5, color: "var(--tx-3)" }}>
                read tools only
              </span>
            )}
            <span style={{ ...mono, color: "var(--tx-3)" }}>{tk.id}</span>
            <span style={{ fontSize: 11.5, color: "var(--tx-3)" }}>used {ago(tk.lastUsedMs, t)}</span>
            <button className="btn btn-sm btn-ghost" data-testid={`nodemcp-revoke-${tk.id}`} disabled={busy !== null} onClick={() => void revoke(tk.id)}>
              Revoke
            </button>
          </div>
        ))}
      </div>

      {/* ---------- recent calls ---------- */}
      {status && status.recentCalls.length > 0 && (
        <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <span className="eyebrow">Recent calls</span>
          {status.recentCalls.slice(0, 10).map((c, i) => (
            <span key={i} style={{ ...mono, color: c.ok ? "var(--tx-3)" : "var(--danger)" }}>
              {ago(c.atMs, t)} · {c.tokenId} · {c.tool ?? c.method} {c.ok ? "" : "(error)"}
            </span>
          ))}
        </div>
      )}

      {error && (
        <span data-testid="nodemcp-error" style={{ fontSize: 12, color: "var(--danger)" }}>
          {error}
        </span>
      )}
    </div>
  );
}
