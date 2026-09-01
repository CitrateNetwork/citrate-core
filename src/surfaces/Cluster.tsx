// =====================================================================
// citrate-core — Cluster (CX-S4 / CL-S2, lane s4)
//
// A group's private P2P cluster, DAEMON-BACKED (the citrate-cluster sidecar over UDS). The surface
// shows the group's authorized peers (only members may join — the RBAC→network boundary) with their
// LIVE connection state + the co-pinned shared-file set, all real daemon state. A peer shows online
// only when it is actually connected (Rule 1 — never a fabricated peer); a lone node meshes with
// no one yet.
// =====================================================================
import { useEffect } from "react";
import { SurfaceProps } from "./shared";
import {
  clusterSlice,
  loadClusterGroups,
  selectClusterGroup,
  clusterGroupLabel,
  joinCluster,
  leaveCluster,
} from "../shell/slices/cluster";

function shortAddr(a: string): string {
  if (!a) return "—";
  return a.length > 14 ? `${a.slice(0, 8)}…${a.slice(-4)}` : a;
}

export function Cluster(_props: SurfaceProps) {
  const st = clusterSlice.use();

  useEffect(() => {
    void loadClusterGroups();
  }, []);

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, maxWidth: 760 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Cluster</span>
        <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--tx-3)" }}>
          a private mesh among your Group's members
        </span>
      </div>

      <p style={{ fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.55, margin: 0 }}>
        A Group's cluster is a private peer-to-peer mesh. Only the Group's members may join it — the
        roster is the admission list, enforced by the cluster daemon. Peers show online as they
        actually connect; a lone node has no one to mesh with yet.
      </p>

      {st.error && (
        <div className="surface" role="alert" style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--bad, #c0392b)", lineHeight: 1.5 }}>
          {st.error}
        </div>
      )}

      {/* group picker */}
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <span style={{ fontSize: 12, color: "var(--tx-3)" }}>Group</span>
        {st.groups.length === 0 ? (
          <span style={{ fontSize: 12, color: "var(--tx-3)" }}>
            No groups yet — create one in Groups first.
          </span>
        ) : (
          <select
            className="input"
            value={st.selectedId ?? ""}
            onChange={(e) => e.target.value && void selectClusterGroup(e.target.value)}
            style={{ fontSize: 12.5, minWidth: 200 }}
          >
            <option value="" disabled>
              Select a group…
            </option>
            {st.groups.map((g) => (
              <option key={g.id} value={g.id}>
                {clusterGroupLabel(g)}
              </option>
            ))}
          </select>
        )}
      </div>

      {st.selectedId && (
        <>
          {/* CONNECT-S4 — Join / Leave the selected group's cluster mesh. Joining contributes your
              storage and keeps you in sync; membership in the group is what authorizes it (RBAC at the
              daemon). `joined` is this session's truth; an un-provisioned daemon surfaces an honest
              error above, never a fake "joined". */}
          {(() => {
            const joined = st.joined.includes(st.selectedId);
            return (
              <div className="surface" style={{ padding: "12px 16px", display: "flex", alignItems: "center", gap: 12 }}>
                <span style={{ flex: 1, minWidth: 0, fontSize: 12.5, color: "var(--tx-2)" }}>
                  {joined ? "You're in this cluster — contributing storage and staying in sync." : "Join to contribute your storage and sync files across the mesh."}
                </span>
                {joined ? (
                  <button className="btn btn-ghost btn-sm" disabled={st.joining} onClick={() => st.selectedId && void leaveCluster(st.selectedId)}>Leave cluster</button>
                ) : (
                  <button className="btn btn-primary btn-sm" disabled={st.joining} onClick={() => st.selectedId && void joinCluster(st.selectedId)}>{st.joining ? "Joining…" : "Join cluster"}</button>
                )}
              </div>
            );
          })()}

          {/* status */}
          <div className="surface" style={{ padding: "14px 16px", display: "flex", gap: 24 }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              <span style={{ fontSize: 20, fontWeight: 540 }}>
                {st.status ? `${st.status.online}/${st.status.total}` : st.loading ? "…" : "—"}
              </span>
              <span style={{ fontSize: 10.5, color: "var(--tx-3)" }}>peers online / authorized</span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
              <span style={{ fontSize: 20, fontWeight: 540 }}>{st.status?.sharedFiles.length ?? 0}</span>
              <span style={{ fontSize: 10.5, color: "var(--tx-3)" }}>shared files</span>
            </div>
          </div>

          {/* peers */}
          <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
            <div style={{ padding: "10px 16px", fontSize: 10.5, color: "var(--tx-3)", textTransform: "uppercase", letterSpacing: 0.4 }}>
              Authorized peers · the mesh admits only these
            </div>
            {st.peers.length === 0 ? (
              <div style={{ padding: "0 16px 14px", fontSize: 12, color: "var(--tx-3)" }}>
                {st.loading ? "Loading…" : "No members in this group's roster yet."}
              </div>
            ) : (
              st.peers.map((p) => (
                <div
                  key={p.address}
                  style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 16px", borderTop: "1px solid var(--ln, rgba(0,0,0,0.06))" }}
                >
                  <span className="mono" style={{ fontSize: 12, flex: 1 }}>{shortAddr(p.address)}</span>
                  <span
                    style={{
                      fontSize: 10.5,
                      color: p.online ? "var(--ok, #2e9e5b)" : "var(--tx-3)",
                    }}
                  >
                    {p.online ? "online" : "authorized · offline"}
                  </span>
                </div>
              ))
            )}
          </div>

          <p style={{ fontSize: 10.5, color: "var(--tx-3)", lineHeight: 1.5, margin: 0 }}>
            These addresses are authorized to join the mesh because they are in the Group. They come
            online as their nodes connect over the peer-to-peer transport, and share files across it.
          </p>
        </>
      )}
    </div>
  );
}
