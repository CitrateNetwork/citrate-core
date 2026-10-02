// =====================================================================
// HUP-S4.4 — Settings > MCP servers: add, edit, remove, and review before
// enabling. See mcpServers.ts for the flow; Rust enforces it (an entry can be
// enabled only with the token of a successful check of the entry as it stands).
// =====================================================================
import { useEffect, useState } from "react";
import {
  draftFromServer,
  draftToInput,
  emptyDraft,
  errorFor,
  toolBadges,
  type BadgeTone,
  type McpDraft,
  type McpFieldError,
  type McpIo,
  type McpReview,
  type McpReviewResult,
  type McpSaveResult,
  type McpServerView,
} from "./mcpServers";

const TONE: Record<BadgeTone, string> = {
  ok: "var(--ok, var(--tx-2))",
  warn: "var(--warn)",
  danger: "var(--danger)",
  muted: "var(--tx-3)",
};

function Badge({ label, tone }: { label: string; tone: BadgeTone }) {
  return (
    <span className="mono" style={{ fontSize: 10, padding: "1px 6px", borderRadius: 999, border: `1px solid ${TONE[tone]}`, color: TONE[tone] }}>
      {label}
    </span>
  );
}

function FieldErr({ errors, field }: { errors: McpFieldError[]; field: string }) {
  const m = errorFor(errors, field);
  if (!m) return null;
  return (
    <span data-testid={`mcp-err-${field}`} role="alert" style={{ fontSize: 11.5, color: "var(--danger)" }}>
      {m}
    </span>
  );
}

const label = { fontSize: 12, color: "var(--tx-2)" } as const;
const note = { fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 } as const;

function where(s: McpServerView): string {
  return s.transport === "stdio" ? [s.command ?? "", ...s.args].join(" ") : (s.url ?? "");
}

export function McpServersPanel({ io }: { io: () => Promise<McpIo> }) {
  const [mode, setMode] = useState<McpIo["mode"] | null>(null);
  const [servers, setServers] = useState<McpServerView[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // The form: null = closed; previous = the entry being edited (null = new).
  const [form, setForm] = useState<{ draft: McpDraft; previous: string | null; errors: McpFieldError[] } | null>(null);
  const [review, setReview] = useState<McpReview | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);

  const run = async <T,>(cmd: string, args: Record<string, unknown>): Promise<T | null> => {
    setBusy(true);
    setError(null);
    try {
      const x = await io();
      return await x.invoke<T>(cmd, args);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return null;
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const x = await io();
        if (!live) return;
        setMode(x.mode);
        if (x.mode !== "tauri") return;
        const list = await x.invoke<McpServerView[]>("mcp_servers_list", {});
        if (live) setServers(list);
      } catch (e) {
        if (live) setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      live = false;
    };
  }, [io]);

  if (mode === "sim") {
    return (
      <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 8 }}>
        <span className="eyebrow">MCP servers</span>
        <span style={note}>Adding your own MCP servers needs the desktop app: the web preview has no agent sidecar to check them.</span>
      </div>
    );
  }

  const save = async () => {
    if (!form) return;
    const r = await run<McpSaveResult>("mcp_server_save", { input: draftToInput(form.draft, form.previous) });
    if (!r) return;
    setServers(r.servers);
    if (r.ok) setForm(null);
    else setForm({ ...form, errors: r.errors });
  };

  const check = async (name: string) => {
    setReview(null);
    const r = await run<McpReviewResult>("mcp_server_review", { name });
    if (!r) return;
    if (r.errors.length > 0) {
      setError(`The agent sidecar refused this entry: ${r.errors.map((e) => `${e.field}: ${e.message}`).join("; ")}`);
      return;
    }
    setReview(r.review);
  };

  const enable = async () => {
    if (!review) return;
    const list = await run<McpServerView[]>("mcp_server_enable", { name: review.server.name, reviewToken: review.reviewToken });
    if (list) {
      setServers(list);
      setReview(null);
    }
  };

  const disable = async (name: string) => {
    const list = await run<McpServerView[]>("mcp_server_disable", { name });
    if (list) setServers(list);
  };

  const remove = async (name: string) => {
    const list = await run<McpServerView[]>("mcp_server_remove", { name });
    setConfirmRemove(null);
    if (list) setServers(list);
  };

  const setDraft = (patch: Partial<McpDraft>) => form && setForm({ ...form, draft: { ...form.draft, ...patch } });

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
        <span className="eyebrow">MCP servers · added by you</span>
        <span style={note}>
          Give Hermes tools from your own MCP servers. A server you add stays off until you check it and look over what it offers. Everything these tools return is
          treated as untrusted data, never as instructions, and tools that change things are not offered unless you turn them on for that server. Changes apply
          the next time Hermes starts.
        </span>
        {error && (
          <span data-testid="mcp-error" role="alert" style={{ fontSize: 12, color: "var(--danger)" }}>
            {error}
          </span>
        )}
        {servers.length === 0 && !form && <span style={note}>No MCP servers added yet.</span>}
        {servers.map((s) => (
          <div key={s.name} style={{ display: "flex", flexDirection: "column", gap: 4, padding: "8px 0", borderTop: "1px solid var(--line, var(--srf-2))" }}>
            <span style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
              <span style={{ fontSize: 13, fontWeight: 500 }}>{s.name}</span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                {s.transport}
              </span>
              {s.enabled ? <Badge label="enabled" tone="ok" /> : <Badge label="needs review" tone="warn" />}
              {s.allowWriteTools && <Badge label="write tools on" tone="danger" />}
            </span>
            <span className="mono" style={{ fontSize: 11, color: "var(--tx-2)", wordBreak: "break-all" }}>
              {where(s)}
            </span>
            {s.env.length > 0 && (
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                {s.env.map((e) => `${e.key} = ${e.masked}`).join(" · ")}
              </span>
            )}
            <span style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
              <button data-testid={`mcp-review-${s.name}`} className="btn btn-secondary btn-sm" disabled={busy} onClick={() => void check(s.name)}>
                {busy && review === null ? "Checking…" : s.enabled ? "Check again" : "Check and review"}
              </button>
              {s.enabled && (
                <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => void disable(s.name)}>
                  Turn off
                </button>
              )}
              <button
                data-testid={`mcp-edit-${s.name}`}
                className="btn btn-ghost btn-sm"
                disabled={busy}
                onClick={() => {
                  setReview(null);
                  setForm({ draft: draftFromServer(s), previous: s.name, errors: [] });
                }}
              >
                Edit
              </button>
              {confirmRemove === s.name ? (
                <>
                  <button data-testid={`mcp-remove-confirm-${s.name}`} className="btn btn-ghost btn-sm" style={{ color: "var(--danger)" }} disabled={busy} onClick={() => void remove(s.name)}>
                    Remove {s.name}
                  </button>
                  <button className="btn btn-ghost btn-sm" onClick={() => setConfirmRemove(null)}>
                    Keep
                  </button>
                </>
              ) : (
                <button data-testid={`mcp-remove-${s.name}`} className="btn btn-ghost btn-sm" disabled={busy} onClick={() => setConfirmRemove(s.name)}>
                  Remove
                </button>
              )}
            </span>
          </div>
        ))}
        {!form && (
          <span>
            <button
              data-testid="mcp-add"
              className="btn btn-secondary btn-sm"
              disabled={busy || mode === null}
              onClick={() => {
                setReview(null);
                setForm({ draft: emptyDraft(), previous: null, errors: [] });
              }}
            >
              Add a server
            </button>
          </span>
        )}
      </div>

      {form && (
        <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
          <span className="eyebrow">{form.previous ? `Edit ${form.previous}` : "Add an MCP server"}</span>
          <FieldErr errors={form.errors} field="entry" />
          <label style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <span style={label}>Name (a-z, 0-9, _ or -; tools appear as mcp__name__tool)</span>
            <input data-testid="mcp-name" className="input" value={form.draft.name} onChange={(e) => setDraft({ name: e.target.value })} style={{ height: 30 }} />
            <FieldErr errors={form.errors} field="name" />
          </label>
          <span style={{ display: "flex", gap: 8 }}>
            {(["stdio", "http"] as const).map((t) => (
              <button key={t} data-testid={`mcp-transport-${t}`} className={"btn btn-sm " + (form.draft.transport === t ? "btn-secondary" : "btn-ghost")} onClick={() => setDraft({ transport: t })}>
                {t === "stdio" ? "A program on this computer" : "A URL"}
              </button>
            ))}
          </span>
          <FieldErr errors={form.errors} field="transport" />
          {form.draft.transport === "stdio" ? (
            <>
              <label style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                <span style={label}>Program (absolute path)</span>
                <input data-testid="mcp-command" className="input mono" placeholder="/usr/local/bin/my-mcp-server" value={form.draft.command} onChange={(e) => setDraft({ command: e.target.value })} style={{ height: 30 }} />
                <FieldErr errors={form.errors} field="command" />
              </label>
              <label style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                <span style={label}>Arguments (one per line)</span>
                <textarea data-testid="mcp-args" className="input mono" rows={3} value={form.draft.argsText} onChange={(e) => setDraft({ argsText: e.target.value })} />
                <FieldErr errors={form.errors} field="args" />
              </label>
              <label style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                <span style={label}>Working folder (optional, absolute path)</span>
                <input data-testid="mcp-cwd" className="input mono" value={form.draft.cwd} onChange={(e) => setDraft({ cwd: e.target.value })} style={{ height: 30 }} />
                <FieldErr errors={form.errors} field="cwd" />
              </label>
              <span style={label}>Environment</span>
              <span style={note}>
                The server gets only these values plus basics like PATH and HOME. Nothing else from your environment is passed on, and values are used exactly as typed.
              </span>
              <FieldErr errors={form.errors} field="env" />
              {form.draft.env.map((e, i) => (
                <span key={i} style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                  <span style={{ display: "flex", gap: 6 }}>
                    <input
                      data-testid={`mcp-env-key-${i}`}
                      className="input mono"
                      placeholder="NAME"
                      value={e.key}
                      onChange={(ev) => setDraft({ env: form.draft.env.map((x, j) => (j === i ? { ...x, key: ev.target.value, keep: false } : x)) })}
                      style={{ height: 30, width: 180 }}
                    />
                    <input
                      data-testid={`mcp-env-value-${i}`}
                      className="input mono"
                      type="password"
                      autoComplete="off"
                      placeholder={e.keep ? "keep the saved value" : "value"}
                      value={e.value}
                      onChange={(ev) => setDraft({ env: form.draft.env.map((x, j) => (j === i ? { ...x, value: ev.target.value } : x)) })}
                      style={{ height: 30, flex: 1 }}
                    />
                    <button className="btn btn-ghost btn-sm" onClick={() => setDraft({ env: form.draft.env.filter((_, j) => j !== i) })}>
                      Remove
                    </button>
                  </span>
                  <FieldErr errors={form.errors} field={`env.${e.key}`} />
                </span>
              ))}
              <span>
                <button data-testid="mcp-env-add" className="btn btn-ghost btn-sm" onClick={() => setDraft({ env: [...form.draft.env, { key: "", value: "", keep: false }] })}>
                  Add a variable
                </button>
              </span>
            </>
          ) : (
            <label style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <span style={label}>URL (https, or http to this computer)</span>
              <input data-testid="mcp-url" className="input mono" placeholder="https://example.com/mcp" value={form.draft.url} onChange={(e) => setDraft({ url: e.target.value })} style={{ height: 30 }} />
              <FieldErr errors={form.errors} field="url" />
            </label>
          )}
          <label style={{ display: "flex", alignItems: "flex-start", gap: 8, fontSize: 12.5 }}>
            <input data-testid="mcp-write" type="checkbox" checked={form.draft.allowWriteTools} onChange={(e) => setDraft({ allowWriteTools: e.target.checked })} />
            <span>
              Offer this server's tools that change things
              <span style={{ ...note, display: "block" }}>Off by default. When off, only tools the server marks read-only are offered to Hermes.</span>
            </span>
          </label>
          <span style={note}>Saving keeps the server off. Check it from the list to review it and turn it on.</span>
          <span style={{ display: "flex", gap: 8 }}>
            <button data-testid="mcp-save" className="btn btn-primary btn-sm" disabled={busy} onClick={() => void save()}>
              {busy ? "Saving…" : "Save"}
            </button>
            <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => setForm(null)}>
              Cancel
            </button>
          </span>
        </div>
      )}

      {review && (
        <div data-testid="mcp-review" className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
          <span className="eyebrow">Review · {review.server.name}</span>
          <span style={label}>{review.server.transport === "stdio" ? "This exact command will run on this computer" : "Hermes will connect to this exact URL"}</span>
          <code data-testid="mcp-review-command" className="mono" style={{ fontSize: 11.5, padding: "8px 10px", background: "var(--srf-1)", borderRadius: "var(--r-1)", wordBreak: "break-all" }}>
            {review.commandLine}
          </code>
          {review.server.cwd && <span className="mono" style={{ fontSize: 11, color: "var(--tx-2)" }}>in {review.server.cwd}</span>}
          {review.server.env.length > 0 && (
            <span style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              <span style={label}>Environment (values hidden)</span>
              {review.server.env.map((e) => (
                <span key={e.key} className="mono" style={{ fontSize: 11 }}>
                  {e.key} = {e.masked}
                </span>
              ))}
            </span>
          )}
          {review.probe.ok ? (
            <>
              <span style={note}>
                {review.probe.serverName || "The server"} {review.probe.serverVersion ?? ""} answered with protocol {review.probe.protocolVersion ?? "?"} and offers{" "}
                {review.probe.tools.length} {review.probe.tools.length === 1 ? "tool" : "tools"}
                {review.probe.toolsTruncated ? " (more were listed than Hermes reads)" : ""}. Every tool below is untrusted: what it returns is data, never instructions.
              </span>
              {review.probe.tools.map((t) => (
                <div key={t.name} style={{ display: "flex", flexDirection: "column", gap: 3, padding: "6px 0", borderTop: "1px solid var(--line, var(--srf-2))" }}>
                  <span style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
                    <span className="mono" style={{ fontSize: 12, fontWeight: 500 }}>
                      {t.name}
                    </span>
                    {toolBadges(t).map((b) => (
                      <Badge key={b.label} label={b.label} tone={b.tone} />
                    ))}
                    <span style={{ fontSize: 11, color: t.offered ? "var(--tx-2)" : "var(--tx-3)" }}>{t.offered ? "offered to Hermes" : "not offered"}</span>
                  </span>
                  {t.description && <span style={{ fontSize: 11.5, color: "var(--tx-2)" }}>{t.description}</span>}
                  {!t.offered && t.skipReason && <span style={note}>{t.skipReason}</span>}
                </div>
              ))}
            </>
          ) : (
            <span role="alert" style={{ fontSize: 12, color: "var(--danger)" }}>
              The check did not complete: {review.probe.error ?? "no reason given"}. Fix the entry or the server, then check again.
            </span>
          )}
          <span style={note}>Turning it on writes it to the list Hermes reads. It takes effect the next time Hermes starts.</span>
          <span style={{ display: "flex", gap: 8 }}>
            <button data-testid="mcp-enable" className="btn btn-primary btn-sm" disabled={busy || !review.canEnable || !review.probe.ok} onClick={() => void enable()}>
              Turn on {review.server.name}
            </button>
            <button className="btn btn-ghost btn-sm" onClick={() => setReview(null)}>
              Close
            </button>
          </span>
        </div>
      )}
    </div>
  );
}
