// =====================================================================
// citrate-core — Connections (CX redesign, Pass 1) · new top-level surface
//
// Everything the node + agent can reach, in one place — built 1:1 from design/CitrateCore.dc.html.
// Five sections, each honest about its backing (Rule 1):
//   • Social identity (X / LinkedIn / Discord) — NOT WIRED; needs a privacy ADR + proof-of-ownership
//     backend. The rows show the intended opt-in → verify → visibility flow and say so plainly.
//   • MCP servers — WIRED via bridge.connections (GitHub / Google Drive / Notion over OAuth PKCE;
//     tokens sealed in the OS keyring), the same seam Settings uses. Real connect/disconnect state.
//   • Foundational models — routes to the real Models + Settings surfaces (no duplicate wiring).
//   • SaaS tools — NOT WIRED; same OAuth/keyring pattern as MCP when it lands.
//   • Webhooks — NOT WIRED; HMAC-signed event posts. A local draft list, flagged as not-yet-delivering.
//
// The companion runbook (docs/CONNECTIONS_RUNBOOK.md) is the buildable checklist for turning every
// "pending backend" row here into a real connection.
// =====================================================================
import { useEffect, useState } from "react";
import { SurfaceProps } from "./shared";
import { bridge } from "../bridge";
import type { ConnectionInfo } from "../bridge/domains";

const SOCIALS: { id: string; name: string; glyph: string; sub: string }[] = [
  { id: "x", name: "X", glyph: "X", sub: "@handle · proves you own the account" },
  { id: "linkedin", name: "LinkedIn", glyph: "in", sub: "professional identity · verified badge" },
  { id: "discord", name: "Discord", glyph: "DC", sub: "username · reachable in your groups" },
];

const MCP_LABEL: Record<string, { name: string; scope: string }> = {
  github: { name: "GitHub", scope: "repos, issues, actions — read + propose" },
  gdrive: { name: "Google Drive", scope: "files your agent can read + summarize" },
  notion: { name: "Notion", scope: "pages + databases — read + write" },
};

const SAAS: { id: string; name: string; scope: string }[] = [
  { id: "slack", name: "Slack", scope: "post + read in channels your agent joins" },
  { id: "linear", name: "Linear", scope: "issues + cycles — read + create" },
  { id: "gcal", name: "Google Calendar", scope: "events — read + propose" },
  { id: "gmail", name: "Gmail", scope: "read + draft (never send without approval)" },
];

export function Connections({ store }: SurfaceProps) {
  const [mcp, setMcp] = useState<ConnectionInfo[]>([]);
  const [mcpBusy, setMcpBusy] = useState<string | null>(null);
  const [social, setSocial] = useState<Record<string, "off" | "verifying">>({});
  const [socialVis, setSocialVis] = useState<"groups" | "private">("groups");
  const [hooks, setHooks] = useState<string[]>([]);
  const [hookUrl, setHookUrl] = useState("");
  const [mcpUrl, setMcpUrl] = useState("");

  const loadMcp = async () => {
    try {
      setMcp(await bridge.connections.status());
    } catch {
      /* honest-empty: connections seam unavailable in this build */
    }
  };
  useEffect(() => {
    void loadMcp();
  }, []);

  const toggleMcp = async (svc: string, connected: boolean) => {
    setMcpBusy(svc);
    try {
      if (connected) await bridge.connections.disconnect(svc);
      else await bridge.connections.start(svc);
      await loadMcp();
    } catch (e) {
      store.toast("Couldn't update the connection — " + (e instanceof Error ? e.message : String(e)));
    } finally {
      setMcpBusy(null);
    }
  };

  const linkSocial = (id: string) => {
    // NOT WIRED: proof-of-ownership + the address↔identity binding need a privacy ADR + backend.
    setSocial((s) => ({ ...s, [id]: "verifying" }));
    store.toast("Linking lands with Social discovery — it verifies you own the account, then binds it per your visibility setting. Pending backend.");
    // Honest: no fabricated "verified" — it stays in the intended "verifying" affordance until wired.
  };

  const addHook = () => {
    const u = hookUrl.trim();
    if (!u) return;
    if (!/^https:\/\//i.test(u)) {
      store.toast("Webhook endpoints must be https:// — plain http isn't accepted.");
      return;
    }
    setHooks((h) => (h.includes(u) ? h : [...h, u]));
    setHookUrl("");
    store.toast("Endpoint saved as a draft — HMAC-signed delivery turns on when the webhook backend ships.");
  };

  const known = new Set(Object.keys(MCP_LABEL));
  const mcpRows = Object.keys(MCP_LABEL).map((id) => {
    const info = mcp.find((c) => c.service === id);
    return { id, ...MCP_LABEL[id], connected: !!info?.connected };
  });
  const extraMcp = mcp.filter((c) => !known.has(c.service));

  const Section = ({ title, flag, children }: { title: string; flag?: string; children: React.ReactNode }) => (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <span className="eyebrow">{title}</span>
        {flag && (
          <span className="mono" style={{ fontSize: 9, letterSpacing: ".1em", textTransform: "uppercase", padding: "2px 8px", borderRadius: 999, border: "1px solid var(--warn)", color: "var(--warn)", background: "var(--warn-bg)" }}>
            {flag}
          </span>
        )}
      </div>
      {children}
    </div>
  );

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, maxWidth: 880 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Connections</span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 10, letterSpacing: ".06em", color: "var(--tx-3)" }}>
          everything your agent can reach — connect once, approve every effect
        </span>
      </div>

      {/* ---- Social identity (NOT WIRED) ---- */}
      <Section title="Social identity" flag="pending backend">
        <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
          {SOCIALS.map((so) => {
            const state = social[so.id] ?? "off";
            return (
              <div key={so.id} style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
                <span className="mono" style={{ width: 28, height: 28, borderRadius: "var(--r-1)", border: "1px solid var(--line-2)", display: "inline-flex", alignItems: "center", justifyContent: "center", fontSize: 10, fontWeight: 600, color: "var(--tx-2)", flexShrink: 0 }}>{so.glyph}</span>
                <span style={{ flex: 1, minWidth: 0 }}>
                  <span style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
                    <span style={{ fontSize: 13, fontWeight: 500 }}>{so.name}</span>
                    {state === "verifying" && <span className="mono" style={{ fontSize: 9, color: "var(--warn)" }}>verifying…</span>}
                  </span>
                  <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 2 }}>{so.sub}</span>
                </span>
                <button className="btn btn-ghost btn-sm" onClick={() => linkSocial(so.id)}>{state === "verifying" ? "Verifying…" : "Link"}</button>
              </div>
            );
          })}
          <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 16px" }}>
            <span style={{ flex: 1, fontSize: 12, color: "var(--tx-2)", lineHeight: 1.55 }}>Who can see your linked identities</span>
            {(["groups", "private"] as const).map((v) => (
              <button key={v} className={"btn btn-sm " + (socialVis === v ? "btn-secondary" : "btn-ghost")} onClick={() => setSocialVis(v)}>
                {v === "groups" ? "Groups only" : "Private"}
              </button>
            ))}
          </div>
        </div>
        <p className="mono" style={{ fontSize: 10, color: "var(--tx-3)", margin: 0, lineHeight: 1.6 }}>
          links are opt-in, verified, and removable · your address-to-identity binding is visible only per the setting above — never published
        </p>
      </Section>

      {/* ---- MCP servers (WIRED via bridge.connections) ---- */}
      <Section title="MCP servers">
        <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
          {mcpRows.map((mc) => (
            <div key={mc.id} style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
              <span style={{ flex: 1, minWidth: 0 }}>
                <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>{mc.name}</span>
                <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 2 }}>{mc.scope}</span>
              </span>
              {mc.connected && <span className="mono" style={{ fontSize: 9.5, color: "var(--ok)" }}>mounted as agent tool</span>}
              <button className="btn btn-ghost btn-sm" disabled={mcpBusy === mc.id} onClick={() => void toggleMcp(mc.id, mc.connected)}>
                {mcpBusy === mc.id ? "…" : mc.connected ? "Disconnect" : "Connect"}
              </button>
            </div>
          ))}
          {extraMcp.map((c) => (
            <div key={c.service} style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
              <span style={{ flex: 1, minWidth: 0 }}>
                <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>{c.service}</span>
                <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 2 }}>{c.scope ?? "connected service"}</span>
              </span>
              {c.connected && <span className="mono" style={{ fontSize: 9.5, color: "var(--ok)" }}>mounted as agent tool</span>}
              <button className="btn btn-ghost btn-sm" disabled={mcpBusy === c.service} onClick={() => void toggleMcp(c.service, c.connected)}>{c.connected ? "Disconnect" : "Connect"}</button>
            </div>
          ))}
          <div style={{ display: "flex", gap: 10, padding: "12px 16px", alignItems: "center" }}>
            <input className="input" value={mcpUrl} onChange={(e) => setMcpUrl(e.target.value)} placeholder="Custom server — stdio command or https:// endpoint" style={{ flex: 1 }} />
            <button className="btn btn-secondary btn-sm" onClick={() => { if (mcpUrl.trim()) { store.toast("Custom MCP servers write to your config — full custom-endpoint wiring is on the runbook."); setMcpUrl(""); } }}>Add server</button>
          </div>
        </div>
        <p className="mono" style={{ fontSize: 10, color: "var(--tx-3)", margin: 0, lineHeight: 1.6 }}>
          OAuth runs on a loopback PKCE flow · tokens seal in your OS keyring, never in the app · connected servers mount as tools your agent proposes with (and you approve)
        </p>
      </Section>

      {/* ---- Foundational models (routes to real surfaces) ---- */}
      <Section title="Foundational models">
        <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
            <span style={{ flex: 1, minWidth: 0 }}>
              <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>Local model</span>
              <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 2 }}>runs on your node · powers chat + your agent</span>
            </span>
            <a href="#/models" onClick={(e) => { e.preventDefault(); store.go("models"); }} className="mono" style={{ fontSize: 10.5, color: "var(--accent-text)", textDecoration: "none" }}>Manage in Models →</a>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 16px" }}>
            <span style={{ flex: 1, minWidth: 0 }}>
              <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>Cloud providers</span>
              <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 2 }}>OpenAI, Anthropic, or your gateway key — sealed in the keyring</span>
            </span>
            <a href="#/settings" onClick={(e) => { e.preventDefault(); store.go("settings"); }} className="mono" style={{ fontSize: 10.5, color: "var(--accent-text)", textDecoration: "none" }}>Manage in Settings →</a>
          </div>
        </div>
      </Section>

      {/* ---- SaaS tools (NOT WIRED) ---- */}
      <Section title="SaaS tools" flag="pending backend">
        <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
          {SAAS.map((sa) => (
            <div key={sa.id} style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
              <span style={{ flex: 1, minWidth: 0 }}>
                <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>{sa.name}</span>
                <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 2 }}>{sa.scope}</span>
              </span>
              <button className="btn btn-ghost btn-sm" onClick={() => store.toast(`${sa.name} connects over the same OAuth + keyring flow as MCP — on the runbook, not wired yet.`)}>Connect</button>
            </div>
          ))}
        </div>
      </Section>

      {/* ---- Webhooks (NOT WIRED) ---- */}
      <Section title="Webhooks" flag="pending backend">
        <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
          {hooks.length === 0 ? (
            <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: 16 }}>
              No endpoints yet. Add one and the app posts signed event notices — ceremony outcomes, node state changes, cluster health — to your own systems.
            </p>
          ) : (
            hooks.map((u) => (
              <div key={u} style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 16px", borderBottom: "1px solid var(--line-1)" }}>
                <span style={{ flex: 1, minWidth: 0 }}>
                  <span className="mono" style={{ display: "block", fontSize: 11.5, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{u}</span>
                  <span className="mono" style={{ display: "block", fontSize: 9.5, color: "var(--tx-3)", marginTop: 1 }}>ceremony · node · cluster · HMAC-signed (draft)</span>
                </span>
                <button className="btn btn-ghost btn-sm" onClick={() => setHooks((h) => h.filter((x) => x !== u))} style={{ color: "var(--tx-3)" }}>Remove</button>
              </div>
            ))
          )}
          <div style={{ display: "flex", gap: 10, padding: "12px 16px", alignItems: "center" }}>
            <input className="input" value={hookUrl} onChange={(e) => setHookUrl(e.target.value)} placeholder="https://ops.example.com/citrate" style={{ flex: 1 }} onKeyDown={(e) => e.key === "Enter" && addHook()} />
            <button className="btn btn-secondary btn-sm" onClick={addHook}>Add endpoint</button>
          </div>
        </div>
      </Section>
    </div>
  );
}
