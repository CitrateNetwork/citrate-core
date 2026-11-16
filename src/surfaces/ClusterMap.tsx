// V60 MESH-4: the cluster map. This machine in the centre, the group's authorized peers around it;
// solid edge = direct connection, dashed = through a Citrate relay, dotted = online but the daemon
// did not say how (an older daemon). Offline peers have no edge. Data: `clusterMapModel(peers)`,
// from the daemon's `peers` answer. Colours are app tokens.
import type { ClusterPeer } from "../bridge/domains";
import { clusterMapModel, MAP_SIZE, type EdgeStyle } from "./clusterMapModel";

const DASH: Record<EdgeStyle, string | undefined> = { solid: undefined, dashed: "6 4", unknown: "1 4" };

const short = (a: string) => (a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a);

export function ClusterMap({ peers }: { peers: ClusterPeer[] }) {
  const m = clusterMapModel(peers);
  const byAddr = new Map(m.nodes.map((n) => [n.address, n]));
  return (
    <figure style={{ margin: 0, padding: "8px 16px 12px" }}>
      <svg
        role="img"
        aria-label={`Cluster map. ${m.summary}`}
        viewBox={`0 0 ${MAP_SIZE} ${MAP_SIZE}`}
        style={{ width: "100%", maxWidth: 320, display: "block", margin: "0 auto" }}
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
              stroke="var(--ok, #2e9e5b)"
              strokeWidth={e.style === "solid" ? 2 : 1.5}
              strokeDasharray={DASH[e.style]}
            />
          );
        })}
        <circle cx={m.center.x} cy={m.center.y} r={10} fill="var(--accent, #c8a200)" />
        <text x={m.center.x} y={m.center.y + 24} textAnchor="middle" fontSize={9} fill="var(--tx-2)">
          This machine
        </text>
        {m.nodes.map((n) => (
          <g key={n.address} data-node={n.address}>
            <circle
              cx={n.x}
              cy={n.y}
              r={7}
              fill={n.online ? "var(--ok, #2e9e5b)" : "none"}
              stroke={n.online ? "var(--ok, #2e9e5b)" : "var(--tx-3)"}
              strokeWidth={1.5}
            >
              <title>{`${n.address}${n.member ? ` (device of ${n.member})` : ""}${n.online ? "" : " · offline"}`}</title>
            </circle>
            <text x={n.x} y={n.y + 18} textAnchor="middle" fontSize={8} fill="var(--tx-3)">
              {short(n.address)}
            </text>
          </g>
        ))}
      </svg>
      <figcaption style={{ fontSize: 11, color: "var(--tx-3)", textAlign: "center", marginTop: 4 }}>
        {m.summary} Solid: direct. Dashed: through a relay.
      </figcaption>
    </figure>
  );
}
