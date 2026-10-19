// =====================================================================
// citrate-core — Store (formerly Commissary; the route id stays `commissary`, K0 owns routes)
//
// K5a: the Store is the SIGNED catalog only (StoreCatalog.tsx → Rust store_catalog.rs),
// verified against the catalog key from the key ceremony, with staged installs that arrive
// paused. Until that key is issued the screen says "Store catalog not available yet".
//
// The local seed tabs (Apps / SDKs / Docs / Services from data/seed.ts CATALOG) are retired
// from under this heading: one Store, one catalog, and nothing listed that did not verify. The
// seed's download button never ran (QA 2026-08-01); the micro-app panel stays reachable from
// the window chrome, and the federation links stay in the docs and the browser.
// =====================================================================
import { SurfaceProps } from "./shared";
import { StoreCatalog } from "./StoreCatalog";

export function Commissary(_props: SurfaceProps) {
  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Store</span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 10, letterSpacing: ".08em", color: "var(--tx-3)" }}>
          signed catalog · first-party listings
        </span>
      </div>
      <StoreCatalog />
    </div>
  );
}
