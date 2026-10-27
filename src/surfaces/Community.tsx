// =====================================================================
// citrate-core — Community (CX redesign, Pass 2) · new surface
//
// The growth surface, built 1:1 from design/CitrateCore.dc.html. It is NOT WIRED — network stats
// and the growth leaderboard need an indexer + a rewards planset before they carry real numbers, so
// this surface shows the STRUCTURE with honest-pending live data (never fabricated counts, Rule 1):
//   • Network stats + leaderboard → "—" / "lands with the indexer" (no invented figures).
//   • The 2,000-run milestone program → real program FACTS (100 / 500 / 2,000 thresholds + reward
//     tiers, settled through the ceremony) — design, not live data, so honest to show.
//   • Inviting someone is done in Groups (one invite flow, D-12): a link they open to ask to join.
// The whole surface carries a "pending backend · illustrative" flag.
// =====================================================================
import { SurfaceProps } from "./shared";

const STATS: { label: string; sub: string }[] = [
  { label: "Nodes online", sub: "counted by the network indexer" },
  { label: "Groups", sub: "from signed roster assertions" },
  { label: "Members", sub: "no self-reported numbers" },
  { label: "Files co-pinned", sub: "held across group clusters" },
];

const MILESTONES: { at: string; reward: string }[] = [
  { at: "100", reward: "founding-roster SALT grant to the whole group" },
  { at: "500", reward: "growth reward + a featured slot in the directory" },
  { at: "2,000", reward: "the full 2,000-run reward, settled to every member" },
];

export function Community({ store }: SurfaceProps) {
  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, maxWidth: 960 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Community</span>
        <span className="mono" style={{ fontSize: 9, letterSpacing: ".1em", textTransform: "uppercase", padding: "2px 8px", borderRadius: 999, border: "1px solid var(--warn)", color: "var(--warn)", background: "var(--warn-bg)" }}>
          pending backend · illustrative
        </span>
      </div>

      {/* network stats — honest pending, no fabricated numbers */}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(4,1fr)", gap: 1, background: "var(--line-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-2)", overflow: "hidden" }}>
        {STATS.map((cs) => (
          <div key={cs.label} style={{ background: "var(--srf-1)", padding: "12px 14px", display: "flex", flexDirection: "column", gap: 4 }}>
            <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" }}>{cs.label}</span>
            <span className="tabular" style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 21, color: "var(--tx-3)" }}>—</span>
            <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>{cs.sub}</span>
          </div>
        ))}
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 320px", gap: 16, alignItems: "start" }}>
        {/* leaderboard — honest empty */}
        <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
          <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
            <span style={{ fontSize: 13.5, fontWeight: 500 }}>Growth leaderboard</span>
            <span className="mono" style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>this epoch</span>
          </div>
          <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: 16 }}>
            Growth standings land with the network indexer. Rankings will be counted from signed roster
            assertions — never self-reported numbers — so what you see here is always real.
          </p>
        </div>

        {/* program + referral */}
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 12 }}>
            <span className="eyebrow">The 2,000 run</span>
            <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>
              Grow a group and the network rewards the whole roster at each milestone. Milestones settle
              each epoch through your ceremony — nothing is paid without your signature.
            </p>
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {MILESTONES.map((cm) => (
                <div key={cm.at} style={{ display: "flex", alignItems: "flex-start", gap: 10 }}>
                  <span className="mono tabular" style={{ width: 44, flexShrink: 0, fontSize: 12, fontWeight: 600, color: "var(--accent-text)", paddingTop: 1 }}>{cm.at}</span>
                  <span style={{ flex: 1, fontSize: 11.5, lineHeight: 1.5, color: "var(--tx-2)" }}>{cm.reward}</span>
                </div>
              ))}
            </div>
            <span className="mono" style={{ fontSize: 9.5, color: "var(--tx-3)", lineHeight: 1.6 }}>
              your group's standing appears here once the indexer lands — no progress bar is drawn from numbers we don't have yet
            </span>
          </div>

          <div className="surface" style={{ padding: "16px 18px", display: "flex", flexDirection: "column", gap: 10 }}>
            <span className="eyebrow">Invite people</span>
            <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>
              To bring someone into a group, open the group in Groups and press Invite someone. They get a one-time link, ask to join, and you approve them.
            </p>
            <button className="btn btn-secondary btn-sm" onClick={() => store.go("groups")} style={{ alignSelf: "flex-start" }}>
              Open Groups
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
