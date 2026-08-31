// =====================================================================
// citrate-core — Connections (CX redesign, Pass 1) · new top-level surface
//
// Everything the node + agent can reach, in one place — built 1:1 from design/CitrateCore.dc.html.
// Five sections, each honest about its backing (Rule 1):
//   • Social identity (X / LinkedIn / Discord) — WIRED via bridge.social (ADR-2026-08-30):
//     loopback-PKCE OAuth ownership proof → keyring-sealed token → wallet-signed IdentityBinding via
//     the ceremony (Rule 3: the wallet signs). CONNECT-S3 repositions it to its REAL job — a verified
//     handle is your FACE to people you share a group with, and the reachability for invite-by-@handle.
//     It is NOT a friend/follower import (X/Discord OAuth doesn't expose that) and the copy says so.
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
import type { ConnectionInfo, LinkedIdentity, SocialNetwork, SocialVisibility } from "../bridge/domains";

// CONNECT-S3 — subcopy states the REAL payoff of linking a network: it becomes your face to people
// you share a group with, and the channel people use to invite you by @handle. Not a friend import.
const SOCIALS: { id: SocialNetwork; name: string; glyph: string; sub: string }[] = [
  { id: "x", name: "X", glyph: "X", sub: "your @handle becomes your face in groups · invite-by-@handle" },
  { id: "linkedin", name: "LinkedIn", glyph: "in", sub: "professional face in groups · verified you own it" },
  { id: "discord", name: "Discord", glyph: "DC", sub: "username as your face in groups · invite-by-@handle" },
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
  const [links, setLinks] = useState<LinkedIdentity[]>([]);
  const [linkBusy, setLinkBusy] = useState<string | null>(null);
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
  const loadSocial = async () => {
    try {
      setLinks(await bridge.social.status());
    } catch {
      /* honest-empty: no links wired yet (Rule 1) */
    }
  };
  useEffect(() => {
    void loadMcp();
    void loadSocial();
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

  // Social identity — wired to bridge.social (ADR-2026-08-30). start() runs the OAuth ownership
  // proof (desktop); the wallet-signed IdentityBinding that makes it "verified" is the follow-up
  // step. On the scaffold, start/setVisibility/disconnect report honest Unavailable ("pending
  // backend") — no fake link is ever shown.
  const linkOf = (network: SocialNetwork): LinkedIdentity | undefined => links.find((l) => l.network === network);
  const linkSocial = async (network: SocialNetwork) => {
    setLinkBusy(network);
    try {
      await bridge.social.start(network);
      await loadSocial();
      store.toast("Account linked — set its visibility, then verify it with your wallet signature.");
    } catch (e) {
      store.toast(e instanceof Error ? e.message : String(e));
    } finally {
      setLinkBusy(null);
    }
  };
  const setVis = async (network: SocialNetwork, visibility: SocialVisibility) => {
    try {
      await bridge.social.setVisibility(network, visibility);
      await loadSocial();
      // Widening to groups shares the verified binding to your groups (server-blind, ADR D1).
      if (visibility === "groups") void store.shareSocialBindings();
    } catch (e) {
      store.toast(e instanceof Error ? e.message : String(e));
    }
  };
  const unlink = async (network: SocialNetwork) => {
    try {
      await bridge.social.disconnect(network);
      await loadSocial();
      store.toast("Unlinked — the binding is dropped and group members are tombstoned.");
    } catch (e) {
      store.toast(e instanceof Error ? e.message : String(e));
    }
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

      {/* ---- Social identity (CONNECT-S3 — wired to bridge.social; repositioned to face + reachability) ---- */}
      <Section title="Social identity">
        <p style={{ fontSize: 12.5, lineHeight: 1.65, color: "var(--tx-2)", margin: "-2px 0 2px" }}>
          Link an account to give yourself a <strong>face</strong>: a verified <span className="mono">@handle</span> that people
          you share a group with can recognize, and the channel they use to <strong>invite you by @handle</strong> instead of a raw address.
        </p>
        <p className="mono" style={{ fontSize: 10, color: "var(--tx-3)", margin: 0, lineHeight: 1.6 }}>
          this does <strong>not</strong> import your followers, friends, or contacts — X and Discord don't share those. Linking only proves the handle is yours.
        </p>
        <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
          {SOCIALS.map((so) => {
            const link = linkOf(so.id);
            const busy = linkBusy === so.id;
            return (
              <div key={so.id} style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
                <span className="mono" style={{ width: 28, height: 28, borderRadius: "var(--r-1)", border: "1px solid var(--line-2)", display: "inline-flex", alignItems: "center", justifyContent: "center", fontSize: 10, fontWeight: 600, color: "var(--tx-2)", flexShrink: 0 }}>{so.glyph}</span>
                <span style={{ flex: 1, minWidth: 0 }}>
                  <span style={{ display: "flex", alignItems: "baseline", gap: 8, flexWrap: "wrap" }}>
                    <span style={{ fontSize: 13, fontWeight: 500 }}>{link ? "@" + link.handle : so.name}</span>
                    {link?.verified && <span className="mono" style={{ fontSize: 9, letterSpacing: ".06em", padding: "1px 7px", borderRadius: 999, border: "1px solid var(--ok)", color: "var(--ok)", background: "var(--ok-bg)" }}>verified · proof of ownership</span>}
                    {link && !link.verified && <span className="mono" style={{ fontSize: 9, color: "var(--warn)" }}>unverified — sign to verify</span>}
                  </span>
                  <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", marginTop: 2 }}>{link ? so.name : so.sub}</span>
                </span>
                {link ? (
                  <>
                    {!link.verified && (
                      <button className="btn btn-secondary btn-sm" onClick={() => void store.verifySocial(so.id, () => { void loadSocial(); void store.shareSocialBindings(); })} title="Sign a wallet challenge to prove you own this account">
                        Verify
                      </button>
                    )}
                    {(["private", "groups"] as const).map((v) => (
                      <button key={v} className={"btn btn-sm " + (link.visibility === v ? "btn-secondary" : "btn-ghost")} onClick={() => void setVis(so.id, v)} title={v === "groups" ? "visible to people who share a group with you" : "visible to no one"}>
                        {v === "groups" ? "Groups" : "Private"}
                      </button>
                    ))}
                    <button className="btn btn-ghost btn-sm" onClick={() => void unlink(so.id)} style={{ color: "var(--tx-3)" }}>Unlink</button>
                  </>
                ) : (
                  <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => void linkSocial(so.id)}>{busy ? "…" : "Link"}</button>
                )}
              </div>
            );
          })}
        </div>
        <p className="mono" style={{ fontSize: 10, color: "var(--tx-3)", margin: 0, lineHeight: 1.6 }}>
          opt-in · private by default · verified with your wallet signature · set a handle to <strong>Groups</strong> and it becomes your face in the People directory and the add-member picker · the address↔identity binding is device-local and shared only to your groups (server-blind), never published (ADR-2026-08-30)
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
