// =====================================================================
// citrate-core — People (CONNECT-S0)
//
// One directory of everyone you share a group with, shown by their verified face (@handle) where one
// resolves, with the groups you have in common and their role in each. Derived LIVE from your group
// rosters + verified social bindings (store.refreshPeople → buildPeopleDirectory) — never fabricated
// (Rule 1): a person with no verified face shows a short address, and with no groups you see an honest
// empty state. Row actions (Add to group / Message) are the S0 affordances the CONNECT arc lights up in
// S2 — they are visibly "next step", never live no-ops.
// =====================================================================
import { useEffect, useMemo, useState } from "react";
import { SurfaceProps } from "./shared";
import { filterPeople, Person } from "./peopleDirectory";

function shortAddr(a: string): string {
  return a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a;
}
function initials(p: Person): string {
  if (p.face) return p.face.handle.slice(0, 2).toUpperCase();
  const hex = p.address.replace(/^0x/i, "");
  return hex.slice(0, 2).toUpperCase();
}
const NET_LABEL: Record<string, string> = { x: "X", discord: "Discord", linkedin: "LinkedIn" };

export function People({ store, s }: SurfaceProps) {
  const [q, setQ] = useState("");

  useEffect(() => {
    void store.refreshPeople();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const shown = useMemo(() => filterPeople(s.people, q), [s.people, q]);
  const loading = s.peopleState === "loading";
  const unavailable = s.peopleState === "unavailable";

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, maxWidth: 900 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>People</span>
        <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", padding: "2px 9px", borderRadius: 999, border: "1px solid var(--line-2)", color: "var(--tx-3)" }}>
          from your groups
        </span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 10, color: "var(--tx-3)" }}>
          {s.peopleState === "ready" ? `${s.people.length} ${s.people.length === 1 ? "person" : "people"}` : ""}
        </span>
      </div>

      <p style={{ fontSize: 12.5, color: "var(--tx-2)", lineHeight: 1.6, margin: 0, maxWidth: 660 }}>
        Everyone you share a group with, in one place — shown by their verified handle when they have one.
        This is your starting point for connecting people into groups and clusters.
      </p>

      <input
        className="input"
        placeholder="Search people by name, handle, address, or group…"
        value={q}
        onChange={(e) => setQ(e.target.value)}
        style={{ maxWidth: 420 }}
      />

      {loading ? (
        <div className="surface" style={{ padding: 18, fontSize: 12.5, color: "var(--tx-3)" }}>Loading people from your groups…</div>
      ) : unavailable ? (
        <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 8 }}>
          <span style={{ fontSize: 13, fontWeight: 500 }}>Couldn't read your groups yet.</span>
          <p style={{ fontSize: 12.5, color: "var(--tx-2)", margin: 0, lineHeight: 1.6 }}>
            People are built from your group rosters. If this persists, the comms daemon isn't reachable — nothing here is invented in the meantime.
          </p>
        </div>
      ) : s.people.length === 0 ? (
        <div className="surface" style={{ padding: 22, display: "flex", flexDirection: "column", gap: 10, alignItems: "flex-start" }}>
          <span style={{ fontSize: 14, fontWeight: 500 }}>No people yet.</span>
          <p style={{ fontSize: 12.5, color: "var(--tx-2)", margin: 0, lineHeight: 1.6, maxWidth: 520 }}>
            When you create or join a group, the people in it show up here — with their verified faces where available. Start by creating a group.
          </p>
          <button className="btn btn-primary" onClick={() => store.go("groups")}>Go to Groups</button>
        </div>
      ) : shown.length === 0 ? (
        <div className="surface" style={{ padding: 18, fontSize: 12.5, color: "var(--tx-3)" }}>No one matches “{q}”.</div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {shown.map((p) => (
            <div key={p.address} className="surface" style={{ display: "flex", alignItems: "center", gap: 14, padding: "12px 16px" }}>
              {/* avatar */}
              <span style={{ width: 34, height: 34, borderRadius: 999, background: "var(--srf-1)", border: "1px solid var(--line-2)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 12, fontWeight: 600, color: "var(--tx-2)", flexShrink: 0 }}>
                {initials(p)}
              </span>
              {/* name + faces + groups */}
              <div style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 0, flex: 1 }}>
                <span style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                  <span style={{ fontSize: 13.5, fontWeight: 500 }}>{p.face ? `@${p.face.handle}` : shortAddr(p.address)}</span>
                  {p.face && (
                    <span className="mono" style={{ fontSize: 9, letterSpacing: ".06em", textTransform: "uppercase", padding: "1px 7px", borderRadius: 999, border: "1px solid var(--ok)", color: "var(--ok)" }}>
                      {NET_LABEL[p.face.network] ?? p.face.network} ✓
                    </span>
                  )}
                  {p.face && <span className="mono" style={{ fontSize: 10, color: "var(--tx-4, var(--tx-3))" }}>{shortAddr(p.address)}</span>}
                </span>
                <span style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                  {p.groups.map((g) => (
                    <span key={g.id} className="mono" style={{ fontSize: 9.5, letterSpacing: ".04em", padding: "2px 8px", borderRadius: 999, background: "var(--srf-1)", border: "1px solid var(--line-1)", color: "var(--tx-3)" }}>
                      {g.name} · {g.role}
                    </span>
                  ))}
                </span>
              </div>
              {/* S0 affordances — visibly next-step, not live no-ops */}
              <span style={{ display: "flex", gap: 8, flexShrink: 0 }}>
                <button className="btn btn-secondary btn-sm" disabled title="One-click add lands in the next step (CONNECT-S2)">Add to group</button>
              </span>
            </div>
          ))}
          <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", marginTop: 2 }}>
            one-click “Add to group / cluster” + messaging arrive in the next step — this list is the foundation
          </span>
        </div>
      )}
    </div>
  );
}
