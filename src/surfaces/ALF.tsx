// =====================================================================
// citrate-core — ALF surface (ALF-ND Phase A). The cooperative WORKBENCH.
//
// "web = trophy case, node = workbench": ownership, earnings, and votes live
// in the ALF web portal (alf.citrate.ai); this surface is where an ALF member
// contributes COMPUTE from the node. It shows the node's real vitals and — until
// the training coordinator + ALF coop are live on chain — an HONEST SEAM for
// contribution (Rule 1: never a fabricated round or unit). No signing here.
//
// Gated by s.alfMember (the alf_member claim). Charter/light surface — App applies
// the register; this file adds no data-register wrapper.
// =====================================================================
import { SurfaceProps } from "./shared";
import { nodeLabel } from "../shell/state";

const ALF_PORTAL = "https://alf.citrate.ai/dashboard";

const eyebrow: React.CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: 11,
  letterSpacing: "0.14em",
  textTransform: "uppercase",
  color: "var(--tx-3)",
};
const card: React.CSSProperties = {
  background: "var(--srf-1)",
  border: "1px solid var(--line-1)",
  borderRadius: "var(--r-2)",
  padding: "20px 22px",
};
const mono: React.CSSProperties = { fontFamily: "var(--font-mono)", fontSize: 12, color: "var(--tx-2)" };

export function ALF({ s }: SurfaceProps) {
  // Defensive: the route is reachable by hash; a non-member gets an honest prompt, never a stub.
  if (!s.alfMember) {
    return (
      <div data-testid="alf-not-member" style={{ padding: "48px 40px", maxWidth: 640 }}>
        <div style={{ ...eyebrow, marginBottom: 10 }}>American Learning Federation</div>
        <h1 style={{ fontFamily: "var(--font-display)", fontSize: 30, margin: "0 0 12px", color: "var(--tx-1)" }}>
          This workbench is for ALF members.
        </h1>
        <p style={{ fontSize: 15, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>
          You&rsquo;re signed in, but this node doesn&rsquo;t see an ALF cooperative membership on your
          identity. Join at <span style={mono}>alf.citrate.ai</span>, then reopen this tab.
        </p>
      </div>
    );
  }

  const nodeUp = s.node !== "off";
  const nodeText = s.node === "off" ? "node off" : s.node === "syncing" ? `syncing ${s.syncPct | 0}%` : `${nodeLabel(s.node)} · ${Math.round(s.height).toLocaleString("en-US")}`;

  return (
    <div data-testid="alf-surface" style={{ padding: "40px 40px 64px", maxWidth: 900 }}>
      <div style={{ ...eyebrow, marginBottom: 10 }}>American Learning Federation</div>
      <h1 style={{ fontFamily: "var(--font-display)", fontSize: 34, letterSpacing: "-0.01em", margin: "0 0 8px", color: "var(--tx-1)" }}>
        Your workbench
      </h1>
      <p style={{ fontSize: 15, lineHeight: 1.6, color: "var(--tx-2)", margin: "0 0 28px", maxWidth: 620 }}>
        This is where you contribute compute to the cooperative from your node. Signing stays here,
        behind a confirmation you approve by hand; your earnings and votes live in the web portal.
      </p>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
        {/* Node vitals — REAL reads from state (honest node status). */}
        <div style={card}>
          <div style={{ ...eyebrow, marginBottom: 12 }}>Your node</div>
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
            <span style={{ width: 8, height: 8, borderRadius: 999, background: nodeUp ? "var(--ok)" : "var(--tx-3)" }} />
            <span data-testid="alf-node-status" style={{ fontFamily: "var(--font-mono)", fontSize: 13, color: "var(--tx-1)" }}>{nodeText}</span>
          </div>
          <div style={mono}>{nodeUp ? `${s.peers | 0} peers` : "start your node to contribute"}</div>
        </div>

        {/* Compute contribution — HONEST SEAM until orchestration + coop are live (Rule 1). */}
        <div style={card} data-testid="alf-compute-seam">
          <div style={{ ...eyebrow, marginBottom: 12 }}>Contribute compute</div>
          <div style={{ display: "inline-flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
            <span style={{ fontFamily: "var(--font-mono)", fontSize: 10.5, letterSpacing: "0.06em", textTransform: "uppercase", color: "var(--warn)", background: "color-mix(in srgb, var(--warn) 14%, transparent)", border: "1px solid var(--warn)", borderRadius: 999, padding: "3px 9px" }}>
              activating soon
            </span>
          </div>
          <p style={{ fontSize: 13.5, lineHeight: 1.55, color: "var(--tx-2)", margin: 0 }}>
            Contributing a training round isn&rsquo;t available yet. It turns on when the training
            coordinator and the ALF cooperative are live on chain. When it does, starting a round will
            ask you to approve a stake here, on your node, by hand. This screen won&rsquo;t fake a round.
          </p>
        </div>
      </div>

      {/* Ownership lives in the web portal (trophy case). */}
      <div style={{ ...card, marginTop: 16 }} data-testid="alf-ownership">
        <div style={{ ...eyebrow, marginBottom: 10 }}>Your ownership</div>
        <p style={{ fontSize: 14, lineHeight: 1.6, color: "var(--tx-2)", margin: "0 0 10px" }}>
          Your patronage, dividends, and votes live in the ALF web portal, where every number is a fact
          you can verify on chain.
        </p>
        <a href={ALF_PORTAL} target="_blank" rel="noreferrer" style={{ ...mono, color: "var(--accent-text)" }}>
          {ALF_PORTAL}
        </a>
      </div>
    </div>
  );
}
