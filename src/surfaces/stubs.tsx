// =====================================================================
// citrate-core — wave-2 surface stubs
// Each surface renders its real title/eyebrow from the design and an honest
// "Building this surface — wave 2" note (Rule 1 / I-3: no half-built surface
// pretends to work). Every stub takes { store, s } — the SurfaceProps
// contract wave-2 agents implement 1:1 against the design's DASHBOARD-peer
// sections. The register is set by App via <main data-register>, matching
// the REGISTER map, so stubs render in the correct register already.
// =====================================================================
import { Store } from "../shell/store";
import { AppState } from "../shell/state";

export interface SurfaceProps {
  store: Store;
  s: AppState;
}

function StubShell({ title, eyebrow, note }: { title: string; eyebrow?: string; note: string }) {
  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, minHeight: "100%", boxSizing: "border-box" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>{title}</span>
        {eyebrow && (
          <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
            {eyebrow}
          </span>
        )}
      </div>
      <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 8, maxWidth: 620 }}>
        <span className="eyebrow">Wave 2</span>
        <p style={{ fontSize: 13, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>{note}</p>
        <p className="mono" style={{ fontSize: 10.5, letterSpacing: ".08em", color: "var(--tx-3)", margin: 0 }}>
          Building this surface — wave 2. The spine, onboarding, dashboard, and global chrome are 1:1; this surface is next.
        </p>
      </div>
    </div>
  );
}

export function Wallet(_: SurfaceProps) {
  return <StubShell title="Wallet" eyebrow={"source · smart wallet"} note="Overview, Staking, Activity, and Identity tabs — balances, send/receive, add/withdraw stake, the client-side signature ledger, and the member SBT panel." />;
}
export function Node(_: SurfaceProps) {
  return <StubShell title="Node" eyebrow={"supervision · node-agent 127.0.0.1:19600"} note="Operations, Earning, and Pinning tabs — start/pause/resume/stop, live log tail, peers, validator stats, resources, earnings breakdown, heartbeat, and the PoSt pinning ledger." />;
}
export function Storage(_: SurfaceProps) {
  return <StubShell title="Storage" eyebrow={"per-user memory graph · encrypted · yours"} note="The 2.5D memory constellation (personal + chain-facts tenants), graph search, lexical/semantic mode, tenant counts, and the MCP endpoint for connecting agents." />;
}
export function Comms(_: SurfaceProps) {
  return <StubShell title="Ping center" note="Notification-only ping center — actor, room, kind, and time; message bodies are never stored here. Polls every 20 seconds, honestly." />;
}
export function Commissary(_: SurfaceProps) {
  return <StubShell title="Commissary" eyebrow={"catalog · signed manifest v3"} note="Apps, SDKs, Docs, and Services tabs — tier-gated cards with signed, checksum-verified downloads, install snippets, and org-scoped doors." />;
}
export function Settings(_: SurfaceProps) {
  return <StubShell title="Settings" note="Account & RBAC, Connections, AI providers, Node configuration, API endpoints & keys, Keys & security, Memberships & billing, and App sections." />;
}
export function Journal(_: SurfaceProps) {
  return <StubShell title="Journal" note="Local, off-chain daily notes and pages — bullet blocks with [[links]], @agent / @prompt tags, backlinks, voice capture, agent chat, encrypted pinning, and export." />;
}
