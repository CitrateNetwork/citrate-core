// =====================================================================
// citrate-core: the FL header band (FL design boards 05 and 06)
//
// Network, wallet, this machine's training worker, and the node in the background. Sources: the
// active network profile; the wallet's eth_getBalance fold (`liquid`, shown "—" until read on
// this network, as the Wallet does); the training worker (not bundled in this build, so it says
// so); the node poll (`node`, `height`). The tokenomics notice is a marked slot pending the
// owner's shared text (design Q6).
// =====================================================================
import type { AppState } from "../shell/state";
import { nodeLabel } from "../shell/state";
import { Eyebrow, Unknown } from "./primitives";

export const TOKENOMICS_NOTICE = "SALT amounts are what contracts escrowed and paid.";

export function FlBand({ s, networkLabel }: { s: Pick<AppState, "liquid" | "netUnread" | "node" | "height">; networkLabel: string }) {
  const balanceRead = !s.netUnread.includes("balances");
  const nodeOn = s.node === "synced" || s.node === "validating";
  return (
    <div className="fl-band" data-testid="fl-band" role="region" aria-label="This machine and network">
      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <Eyebrow>Network</Eyebrow>
        <span style={{ fontSize: 13 }}>{networkLabel}</span>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <Eyebrow>Wallet</Eyebrow>
        <span className="mono" style={{ fontSize: 13 }} title="Read from eth_getBalance">
          {balanceRead ? `${s.liquid.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })} SALT` : <Unknown reason="not read yet on this network" />}
        </span>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <Eyebrow>Training worker</Eyebrow>
        <span style={{ fontSize: 13 }}>Not bundled in this release</span>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <Eyebrow>In the background</Eyebrow>
        <span style={{ fontSize: 13, display: "flex", gap: 6, alignItems: "center" }}>
          <span aria-hidden="true" style={{ width: 7, height: 7, borderRadius: 999, background: nodeOn ? "var(--accent)" : "var(--tx-3)" }} />
          Node {nodeLabel(s.node)}
          {s.height > 0 ? ` · block ${s.height.toLocaleString("en-US")}` : ""}
        </span>
      </div>
      <span style={{ flex: 1 }} />
      <span style={{ fontSize: 11.5, maxWidth: 360, opacity: 0.8 }}>{TOKENOMICS_NOTICE}</span>
    </div>
  );
}
