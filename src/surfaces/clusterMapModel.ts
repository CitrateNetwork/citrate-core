// V60 MESH-4: the cluster map model (03 §UX "Mesh"). This machine sits in the centre; every
// authorized peer of the selected group is a node on a circle around it; an online peer gets an
// edge, solid when the daemon reports a direct connection, dashed when it goes through a relay, and
// "unknown" when the daemon does not say (an older daemon). Offline peers have no edge.
// Source: the cluster daemon's `peers` answer (`cluster_peers`). Nothing here is invented: no
// peer, edge or path exists unless the daemon reported it.
import type { ClusterPeer } from "../bridge/domains";

export type EdgeStyle = "solid" | "dashed" | "unknown";

export interface MapNode {
  address: string;
  online: boolean;
  member?: string;
  x: number;
  y: number;
}

export interface ClusterMapModel {
  center: { x: number; y: number };
  radius: number;
  nodes: MapNode[];
  edges: { to: string; style: EdgeStyle }[];
  summary: string;
}

/** The map's coordinate box (an SVG viewBox of SIZE × SIZE). */
export const MAP_SIZE = 240;

export function clusterMapModel(peers: ClusterPeer[]): ClusterMapModel {
  const center = { x: MAP_SIZE / 2, y: MAP_SIZE / 2 };
  const radius = MAP_SIZE / 2 - 28;
  const sorted = [...peers].sort((a, b) => (a.address < b.address ? -1 : a.address > b.address ? 1 : 0));
  const nodes: MapNode[] = sorted.map((p, i) => {
    // Start at the top and go clockwise, evenly spaced.
    const angle = -Math.PI / 2 + (2 * Math.PI * i) / Math.max(sorted.length, 1);
    return {
      address: p.address,
      online: p.online,
      ...(p.member ? { member: p.member } : {}),
      x: center.x + radius * Math.cos(angle),
      y: center.y + radius * Math.sin(angle),
    };
  });
  const edges = sorted
    .filter((p) => p.online)
    .map((p) => ({
      to: p.address,
      style: (p.link === "direct" ? "solid" : p.link === "relayed" ? "dashed" : "unknown") as EdgeStyle,
    }));
  return { center, radius, nodes, edges, summary: summarize(sorted.length, edges) };
}

function summarize(total: number, edges: { style: EdgeStyle }[]): string {
  if (total === 0) return "No other machines in this group yet.";
  const n = (s: EdgeStyle) => edges.filter((e) => e.style === s).length;
  const parts = [
    n("solid") ? `${n("solid")} direct` : null,
    n("dashed") ? `${n("dashed")} through a relay` : null,
    n("unknown") ? `${n("unknown")} path not reported` : null,
  ].filter(Boolean);
  const head = `${edges.length} of ${total} online`;
  return parts.length ? `${head}: ${parts.join(", ")}.` : `${head}.`;
}
