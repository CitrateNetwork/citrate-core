// V60 MESH-4 + 0.5.4 FL design (board 04): the cluster map. This machine in the centre, the group's
// authorized peers around it; solid edge = direct connection, dashed = through a Citrate relay,
// dotted = online but the daemon did not say how (an older daemon). Offline peers have no edge, and
// with the mesh off there are no live edges. A dotted ring marks a machine that has not reported its
// hardware. Nodes never drift: positions depend only on the peer set. Nothing animates; an edge
// changes only when `cluster_peers` or the mesh status changes. Colours are app tokens; the
// `--mesh-edge-*` tokens come from the FL token set, with app fallbacks until it lands.
import type { ClusterPeer } from "../bridge/domains";
import { clusterMapModel, MAP_SIZE, type EdgeStyle } from "./clusterMapModel";

const DASH: Record<EdgeStyle, string | undefined> = { solid: undefined, dashed: "6 4", unknown: "1 4" };
const EDGE: Record<EdgeStyle, string> = {
  solid: "var(--mesh-edge-direct, var(--ok))",
  dashed: "var(--mesh-edge-relayed, var(--tx-2))",
  unknown: "var(--mesh-edge-relayed, var(--tx-2))",
};

/** The id for this machine's node in `selected` / `onSelect`. */
export const THIS_MACHINE = "self";

const short = (a: string) => (a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a);

export interface ClusterMapProps {
  peers: ClusterPeer[];
  /** Mesh on for this device (default on). */
  meshOn?: boolean;
  /** Peers whose hardware report reached us (none today). */
  reported?: ReadonlySet<string>;
  /** The selected node: a peer address or THIS_MACHINE. */
  selected?: string | null;
  /** Labels for peers (a linked device's name), by address. */
  labels?: ReadonlyMap<string, string>;
}

export function ClusterMap({ peers, meshOn = true, reported, selected = null, labels }: ClusterMapProps) {
  const m = clusterMapModel(peers, { meshOn, reported });
  const byAddr = new Map(m.nodes.map((n) => [n.address, n]));
  const selfOn = selected === THIS_MACHINE;
  return (
    <figure style={{ margin: 0, padding: "8px 16px 12px" }}>
      <svg
        role="img"
        aria-label={m.sentence}
        viewBox={`0 0 ${MAP_SIZE} ${MAP_SIZE}`}
        style={{ width: "100%", maxWidth: 360, display: "block", margin: "0 auto" }}
      >
        {m.edges.map((e) => {
          const n = byAddr.get(e.to);
          if (!n) return null;
          return (
            <line
              key={e.to}
              data-edge={e.to}
              data-style={e.style}
              x1={m.center.x}
              y1={m.center.y}
              x2={n.x}
              y2={n.y}
              stroke={EDGE[e.style]}
              strokeWidth={e.style === "solid" ? 2 : 1.5}
              strokeDasharray={DASH[e.style]}
            />
          );
        })}
        <circle
          data-self="yes"
          cx={m.center.x}
          cy={m.center.y}
          r={selfOn ? 12 : 10}
          fill="var(--accent)"
          stroke={selfOn ? "var(--tx-1)" : "none"}
          strokeWidth={selfOn ? 2 : 0}
        />
        <text x={m.center.x} y={m.center.y + 24} textAnchor="middle" fontSize={9} fill="var(--tx-2)">
          This machine
        </text>
        {m.nodes.map((n) => {
          const on = selected === n.address;
          return (
            <g key={n.address} data-node={n.address} data-reported={n.reported ? "yes" : "no"}>
              <circle
                cx={n.x}
                cy={n.y}
                r={on ? 9 : 7}
                fill={n.online && meshOn ? "var(--ok)" : "none"}
                stroke={on ? "var(--accent)" : n.online && meshOn ? "var(--ok)" : "var(--tx-3)"}
                strokeWidth={on ? 2.5 : 1.5}
                strokeDasharray={n.reported ? undefined : "2 2"}
              >
                <title>{`${labels?.get(n.address) ?? n.address}${n.member ? ` (device of ${n.member})` : ""}${n.online ? "" : " · offline"}${n.reported ? "" : " · hardware not reported"}`}</title>
              </circle>
              <text x={n.x} y={n.y + 19} textAnchor="middle" fontSize={8} fill="var(--tx-3)">
                {labels?.get(n.address) ?? short(n.address)}
              </text>
            </g>
          );
        })}
      </svg>
      <figcaption style={{ fontSize: 11, color: "var(--tx-3)", textAlign: "center", marginTop: 4, lineHeight: 1.5 }}>
        {m.summary}
        <span aria-hidden="true" className="mono" style={{ display: "flex", gap: 14, justifyContent: "center", marginTop: 4, fontSize: 10.5 }}>
          <span>━ direct</span>
          <span>┅ relayed</span>
          <span>◌ not reported</span>
        </span>
      </figcaption>
    </figure>
  );
}
