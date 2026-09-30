// =====================================================================
// citrate-core — blocking notice when this build targets a retired network
//
// Shown only on a definite genesis mismatch (see networkCompat.ts). Unlike the
// update banner this IS blocking: every on-chain surface in a retired build
// would read and sign against dead addresses. The one way forward is the update:
// the overlay sits just BELOW UpdateBanner (z 60), so the existing signed-updater
// card stays clickable on top of it. It deliberately does not start a second
// updater instance (useAppUpdate holds per-instance state).
// =====================================================================
import { useNetworkCompat, EXPECTED_GENESIS } from "./networkCompat";

const short = (h: string | null) => (h ? `${h.slice(0, 10)}…${h.slice(-6)}` : "unknown");

export function NetworkCompatGate() {
  const { status, live } = useNetworkCompat();
  if (status !== "retired") return null;

  return (
    <div
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="netcompat-title"
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 55,
        background: "rgba(0,0,0,0.6)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        padding: 16,
      }}
    >
      <div
        style={{
          width: "min(460px, 100%)",
          background: "var(--srf-1)",
          border: "1px solid var(--bd-1)",
          borderLeft: "3px solid var(--danger)",
          borderRadius: 12,
          padding: "20px 22px",
          color: "var(--tx-1)",
          display: "flex",
          flexDirection: "column",
          gap: 12,
        }}
      >
        <h2 id="netcompat-title" style={{ margin: 0, fontSize: 16 }}>
          Update required
        </h2>
        <p style={{ margin: 0, fontSize: 13, lineHeight: 1.55, color: "var(--tx-2)" }}>
          The Citrate network was restarted with a new genesis. This version of Citrate Core was
          built for the previous network, so its contract addresses no longer exist. Update to keep
          using your node, wallet and membership.
        </p>
        <p className="mono" style={{ margin: 0, fontSize: 11, color: "var(--tx-3)" }}>
          this build: {short(EXPECTED_GENESIS)} · live network: {short(live)}
        </p>
        <p style={{ margin: 0, fontSize: 12.5, lineHeight: 1.5 }}>
          Use the update card in the corner, or download the latest version from{" "}
          <a href="https://citrate.ai/download" target="_blank" rel="noreferrer">
            citrate.ai/download
          </a>
          .
        </p>
      </div>
    </div>
  );
}
