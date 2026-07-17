import markWhite from "../assets/brand/citrate_mark_white.svg";
import marqueeWhite from "../assets/brand/citrate_marquee_white.svg";
import { Store } from "./store";
import { AppState, nodeLabel } from "./state";
import { SbtArt } from "../identity/SbtEmblem";

const fmtI = (n: number) => Math.round(n).toLocaleString("en-US");

// [id, label, iconA, iconB] — verbatim from the design's NAVI list.
export const NAVI: [string, string, string, string][] = [
  ["dashboard", "Dashboard", "M3 11 L12 3 L21 11 V20 H14 V14 H10 V20 H3 Z", "M0 0"],
  ["wallet", "Wallet", "M3 7 V17 A2 2 0 0 0 5 19 H19 A2 2 0 0 0 21 17 V9 A2 2 0 0 0 19 7 Z M3 7 A2 2 0 0 1 5 5 H16", "M15.5 13 H17.5"],
  ["node", "Node", "M4 5 H20 V11 H4 Z M4 13 H20 V19 H4 Z", "M7 8 H7.01 M7 16 H7.01"],
  ["storage", "Storage", "M12 3 C7 3 4 4.5 4 6.5 C4 8.5 7 10 12 10 C17 10 20 8.5 20 6.5 C20 4.5 17 3 12 3 Z", "M4 6.5 V17.5 C4 19.5 7 21 12 21 C17 21 20 19.5 20 17.5 V6.5 M4 12 C4 14 7 15.5 12 15.5 C17 15.5 20 14 20 12"],
  ["journal", "Journal", "M6 3 H19 V21 H6 A2 2 0 0 1 4 19 V5 A2 2 0 0 1 6 3 Z", "M9 3 V21 M12.5 8 H16 M12.5 12 H15"],
  ["comms", "Comms", "M18 16 V11 A6 6 0 0 0 6 11 V16 L4 18 H20 Z", "M10 21 A2 2 0 0 0 14 21"],
  ["commissary", "Commissary", "M21 8 L12 3 L3 8 V16 L12 21 L21 16 Z", "M3 8 L12 13 L21 8 M12 13 V21"],
  ["settings", "Settings", "M4 7 H20 M4 12 H20 M4 17 H20", "M9 5 V9 M15 10 V14 M8 15 V19"],
];

const nodeColors: Record<string, string> = {
  off: "var(--tx-3)",
  prov: "#ffbd10",
  syncing: "#ffbd10",
  synced: "#8ecc09",
  paused: "#ffbd10",
  validating: "#8ecc09",
  error: "#dd7259",
};

export function Sidebar({ store, s }: { store: Store; s: AppState }) {
  const P = store.identity();
  const effTier = s.entitlement === "lapsed" ? "free" : s.tier;
  const tierText = effTier === "free" ? "public tier" : effTier === "enterprise" ? "enterprise · " + (s.org || "") : "pilot member";
  const tierColor = s.entitlement === "active" ? "rgba(205,231,214,.6)" : s.entitlement === "lapsed" ? "#dd7259" : "#ffbd10";
  const sideDotColor = s.node === "off" ? "rgba(205,231,214,.35)" : nodeColors[s.node];
  const sideDotAnim = s.node === "validating" || s.node === "syncing" ? "ccPulse 2.4s var(--ease-standard) infinite" : "none";
  const sideNodeText = s.node === "off" ? "node off" : s.node === "syncing" ? "syncing " + (s.syncPct | 0) + "%" : nodeLabel(s.node) + " · " + fmtI(s.height);

  return (
    <aside style={{ background: "var(--deep-evergreen)", color: "#cde7d6", display: "flex", flexDirection: "column", padding: "18px 12px 12px", minHeight: 0 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "2px 8px 20px" }}>
        <img src={markWhite} alt="" style={{ width: 26, height: 26 }} />
        <img src={marqueeWhite} alt="Citrate" style={{ height: 13 }} />
        <span className="mono" style={{ marginLeft: "auto", fontSize: 9, letterSpacing: ".1em", color: "rgba(205,231,214,.45)" }}>
          CORE
        </span>
      </div>
      <nav style={{ flex: 1, minHeight: 0, overflow: "auto" }}>
        <ul style={{ listStyle: "none", padding: 0, margin: 0, display: "flex", flexDirection: "column", gap: 2 }}>
          {NAVI.map(([id, label, iconA, iconB]) => {
            const active = s.route === id;
            const dot = id === "comms" && s.stage === "done";
            return (
              <li key={id}>
                <a
                  href={"#/" + id}
                  onClick={(e) => {
                    e.preventDefault();
                    store.go(id);
                  }}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 12,
                    padding: "9px 12px",
                    borderRadius: "var(--r-1)",
                    fontSize: 13,
                    fontWeight: active ? 500 : 400,
                    color: active ? "#0e0f0c" : "#cde7d6",
                    background: active ? "var(--citrate-green)" : "transparent",
                    transition: "background var(--dur-fast) var(--ease-standard)",
                    textDecoration: "none",
                  }}
                >
                  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }}>
                    <path d={iconA}></path>
                    <path d={iconB}></path>
                  </svg>
                  <span style={{ flex: 1 }}>{label}</span>
                  {dot && <span style={{ width: 6, height: 6, borderRadius: 999, background: "var(--citrate-yellow)" }}></span>}
                </a>
              </li>
            );
          })}
        </ul>
      </nav>
      <div style={{ borderTop: "1px solid rgba(205,231,214,.12)", padding: "12px 8px 4px", display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ width: 7, height: 7, borderRadius: 999, background: sideDotColor, animation: sideDotAnim, flexShrink: 0 }}></span>
          <span className="mono tabular" style={{ fontSize: 10.5, letterSpacing: ".06em", color: "rgba(205,231,214,.75)" }}>
            {sideNodeText}
          </span>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          {s.hasSbt && s.walletAddr ? (
            // Members with a minted SBT show their deterministic identity emblem
            // (seeded from the on-chain wallet address). Non-members keep initials.
            <SbtArt seed={s.walletAddr} size={28} title="Your membership identity emblem" />
          ) : (
            <span style={{ width: 28, height: 28, borderRadius: 999, background: "var(--citrate-green)", color: "#0e0f0c", display: "inline-flex", alignItems: "center", justifyContent: "center", fontWeight: 600, fontSize: 11, flexShrink: 0 }}>
              {P.initials}
            </span>
          )}
          <span style={{ flex: 1, minWidth: 0 }}>
            <span style={{ display: "block", fontSize: 12.5, fontWeight: 500, color: "#f4f1ea", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{P.name}</span>
            <span className="mono" style={{ display: "block", fontSize: 9, letterSpacing: ".1em", textTransform: "uppercase", color: tierColor }}>
              {tierText}
            </span>
          </span>
        </div>
      </div>
    </aside>
  );
}
