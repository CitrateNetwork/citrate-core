// =====================================================================
// citrate-core — shared surface contract
// Every app surface is a function component taking { store, s }. The
// register is applied by App via <main data-register>, matching the
// REGISTER map, so surfaces do NOT set their own register wrapper.
// StubShell renders an honest "wave 2" placeholder (Rule 1 / I-3) until a
// surface is built 1:1 from design/CitrateCore.dc.html.
// =====================================================================
import { Store } from "../shell/store";
import { AppState } from "../shell/state";

export interface SurfaceProps {
  store: Store;
  s: AppState;
}

export function StubShell({ title, eyebrow, note }: { title: string; eyebrow?: string; note: string }) {
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
