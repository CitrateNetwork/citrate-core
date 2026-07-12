// =====================================================================
// citrate-core — Comms (Ping center)
// Ported 1:1 from design/CitrateCore.dc.html (COMMS section + inline
// script: pollStr, pingRows). Notification-only: actor, room, kind, time —
// message bodies are NEVER stored or shown here. Polls every 20 seconds,
// honestly (pollIn drains in the sim tick, store.ts).
//
// Data source — comms notifications API (actor/room/kind/time only). The
// PINGS list is prototype sim state (data/seed.ts); wiring replaces the sim,
// not the UI (Rule 1). No message bodies ever pass through this surface.
// =====================================================================
import { SurfaceProps } from "./shared";
import { PINGS } from "../data/seed";

export function Comms({ store, s }: SurfaceProps) {
  const pollStr = Math.ceil(s.pollIn) + "s";
  const pingRows = PINGS.map((p) => ({
    key: p.id,
    initial: p.actor[0].toUpperCase(),
    actor: p.actor,
    note: p.note,
    room: p.room,
    ago: p.ago,
    open: () => store.toast("Deep-links to the room — comms web, or the native app if the Commissary installed it"),
  }));

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, maxWidth: 760 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Ping center</span>
        <span className="mono tabular" style={{ marginLeft: "auto", fontSize: 10.5, color: "var(--tx-3)" }}>
          next poll in {pollStr}
        </span>
      </div>
      <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
        {pingRows.map((pg) => (
          <div key={pg.key} style={{ display: "flex", alignItems: "center", gap: 14, padding: "13px 18px", borderBottom: "1px solid var(--line-1)" }}>
            <span
              style={{
                width: 30,
                height: 30,
                borderRadius: 999,
                background: "var(--srf-inset)",
                border: "1px solid var(--line-1)",
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                fontSize: 11,
                fontWeight: 600,
                color: "var(--tx-2)",
                flexShrink: 0,
              }}
            >
              {pg.initial}
            </span>
            <span style={{ flex: 1, minWidth: 0 }}>
              <span style={{ display: "block", fontSize: 13 }}>
                <span style={{ fontWeight: 500 }}>{pg.actor}</span> {pg.note}
              </span>
              <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 2 }}>
                {pg.room} · {pg.ago}
              </span>
            </span>
            <button className="btn btn-ghost btn-sm" onClick={pg.open}>
              Open ↗
            </button>
          </div>
        ))}
      </div>
      <p style={{ fontSize: 11.5, lineHeight: 1.6, color: "var(--tx-3)", margin: 0 }}>
        Message bodies are never stored or shown here — the notifications API carries actor, room, kind, and time only. New
        mentions fire an OS notification. Live push is an upstream milestone; this center polls every 20 seconds, honestly.
      </p>
    </div>
  );
}
