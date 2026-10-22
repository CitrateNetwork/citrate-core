import marqueeWhite from "../assets/brand/citrate_marquee_white.svg";
import { Store } from "./store";
import { AppState, nodeLabel } from "./state";
import { OnChainSbtEmblem } from "../identity/SbtEmblem";
import { agentSlice } from "./slices/agent";
import { PLACES, ROUTABLE, locate } from "./places";

const fmtI = (n: number) => Math.round(n).toLocaleString("en-US");

// Five-place navigation (0.5.2) — Hermes, Groups, Wallet, Node, Store, then Settings. Supersedes the
// HUP-S0.8 sectioned sidebar. No surface was removed: every old route id still opens, inside the
// place that now holds it (src/shell/places.ts; docs/nav/FIVE_PLACE_ROUTE_MAP.md). ALF is a Groups
// tab for members, not a sidebar item.

/** Every deep-linkable route (the hash router's allowlist): canonical routes plus every old id. */
export const ROUTES: string[] = ROUTABLE;

/** The sidebar place a route highlights (an old id lights up the place that now holds it). */
export const navParent = (route: string): string => locate(route).place;

const nodeColors: Record<string, string> = {
  off: "var(--tx-3)",
  prov: "#ffbd10",
  syncing: "#ffbd10",
  synced: "#8ecc09",
  paused: "#ffbd10",
  validating: "#8ecc09",
  stalled: "#e8843c",
  error: "#dd7259",
};

export function Sidebar({ store, s }: { store: Store; s: AppState }) {
  const P = store.identity();
  // The Hermes item carries a live count of approvals awaiting the human, so a pending
  // action is never something the owner has to go hunting for (AGENT-US-4).
  const agentPending = agentSlice.use().approvals.length;
  const here = navParent(s.route);
  const effTier = s.entitlement === "lapsed" ? "free" : s.tier;
  const tierText = effTier === "free" ? "public tier" : effTier === "enterprise" ? "enterprise · " + (s.org || "") : "pilot member";
  const tierColor = s.entitlement === "active" ? "rgba(205,231,214,.6)" : s.entitlement === "lapsed" ? "#dd7259" : "#ffbd10";
  const sideDotColor = s.node === "off" ? "rgba(205,231,214,.35)" : nodeColors[s.node];
  const sideDotAnim = s.node === "validating" || s.node === "syncing" ? "ccPulse 2.4s var(--ease-standard) infinite" : "none";
  const sideNodeText = s.node === "off" ? "node off" : s.node === "syncing" ? "syncing " + (s.syncPct | 0) + "%" : nodeLabel(s.node) + " · " + fmtI(s.height);

  return (
    <aside style={{ background: "var(--deep-evergreen)", color: "#cde7d6", display: "flex", flexDirection: "column", padding: "18px 12px 12px", minHeight: 0 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "2px 8px 20px" }}>
        <img src={marqueeWhite} alt="Citrate" style={{ height: 24 }} />
        <span className="mono" style={{ marginLeft: "auto", fontSize: 9, letterSpacing: ".1em", color: "rgba(205,231,214,.45)" }}>
          CORE
        </span>
      </div>
      <nav style={{ flex: 1, minHeight: 0, overflow: "auto", display: "flex", flexDirection: "column", gap: 10 }}>
        {[PLACES.filter((p) => p.id !== "settings"), PLACES.filter((p) => p.id === "settings")].map((group, gi) => (
          <ul key={gi} style={{ listStyle: "none", padding: 0, margin: gi ? "auto 0 0" : 0, display: "flex", flexDirection: "column", gap: 2 }}>
            {group.map((place) => {
              const id = place.id;
              const [iconA, iconB] = place.icon;
              const href = place.tabs[0].route;
              const active = here === id;
              // Group alerts surface as a dot on Groups; approvals awaiting the human badge Hermes.
              const dot = id === "groups" && s.stage === "done";
              const badge = id === "hermes" && agentPending > 0 ? agentPending : 0;
              return (
                <li key={id}>
                  <a
                    href={"#/" + href}
                    aria-current={active ? "page" : undefined}
                    onClick={(e) => {
                      e.preventDefault();
                      store.go(href);
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
                    <span style={{ flex: 1 }}>{place.label}</span>
                    {badge > 0 && (
                      <span aria-label={badge + " awaiting your review"} className="mono tabular" style={{ fontSize: 9.5, minWidth: 16, height: 16, padding: "0 4px", borderRadius: 999, background: "var(--citrate-yellow)", color: "var(--on-highlight)", display: "inline-flex", alignItems: "center", justifyContent: "center", fontWeight: 600 }}>
                        {badge}
                      </span>
                    )}
                    {dot && badge === 0 && <span style={{ width: 6, height: 6, borderRadius: 999, background: "var(--citrate-yellow)" }}></span>}
                  </a>
                </li>
              );
            })}
          </ul>
        ))}
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
            // Members with a minted SBT show their AUTHORITATIVE on-chain emblem
            // (CitrateMemberSBT tokenURI, resolved from keccak256(sub)); the local
            // deterministic emblem (seeded from the wallet address) is the honest
            // offline fallback. Non-members keep initials.
            <OnChainSbtEmblem sub={P.sub} seed={s.walletAddr} size={28} title="Your membership identity emblem" />
          ) : (
            <span style={{ width: 28, height: 28, borderRadius: 999, background: "var(--citrate-green)", color: "var(--on-accent)", display: "inline-flex", alignItems: "center", justifyContent: "center", fontWeight: 600, fontSize: 11, flexShrink: 0 }}>
              {P.initials}
            </span>
          )}
          <span style={{ flex: 1, minWidth: 0 }}>
            <span style={{ display: "block", fontSize: 12.5, fontWeight: 500, color: "#f4f1ea", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{P.name}</span>
            <span className="mono" style={{ display: "block", fontSize: 9, letterSpacing: ".1em", textTransform: "uppercase", color: tierColor }}>
              {tierText}
            </span>
          </span>
          {/* Build version — dim + unobtrusive, tucked beside the profile so it never crowds. */}
          <span className="mono tabular" title="App version" style={{ fontSize: 9, letterSpacing: ".04em", color: "rgba(205,231,214,.4)", flexShrink: 0, alignSelf: "flex-end" }}>
            v{typeof __APP_VERSION__ === "string" ? __APP_VERSION__ : "0.0.0"}
          </span>
        </div>
      </div>
    </aside>
  );
}
