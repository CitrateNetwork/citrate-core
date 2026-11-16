// V60 MESH-4: the cluster map model (03 §UX "Mesh"). This machine sits in the centre; every
// authorized peer of the selected group is a node on a circle around it; an online peer gets an
// edge, solid when the daemon reports a direct connection, dashed when it goes through a relay, and
// "unknown" when the daemon does not say (an older daemon). Offline peers have no edge.
// Source: the cluster daemon's `peers` answer (`cluster_peers`). Nothing here is invented: no
// peer, edge or path exists unless the daemon reported it.
//
// 0.5.4 FL design (board 04): a node is "reported" only when its machine reported its hardware.
// Members' machines do not share hardware reports today, so every peer is not reported (a dotted
// ring on the map). With the mesh off on this device there are no live edges at all.
import type { ClusterPeer } from "../bridge/domains";

export type EdgeStyle = "solid" | "dashed" | "unknown";

export interface MapNode {
  address: string;
  online: boolean;
  member?: string;
  /** This machine's hardware report reached us. False for every peer today (no source). */
  reported: boolean;
  x: number;
  y: number;
}

export interface ClusterMapModel {
  center: { x: number; y: number };
  radius: number;
  nodes: MapNode[];
  edges: { to: string; style: EdgeStyle }[];
  summary: string;
  /** The full text equivalent: "Cluster map: N devices, X direct links, Y relayed, Z not reported." */
  sentence: string;
}

/** The map's coordinate box (an SVG viewBox of SIZE × SIZE). */
export const MAP_SIZE = 240;

export interface ClusterMapOptions {
  /** The cross-machine mesh is on for this device. Off: no live edges. Default on (older callers). */
  meshOn?: boolean;
  /** Peer addresses whose hardware report reached us. Default none (no source today). */
  reported?: ReadonlySet<string>;
}

export function clusterMapModel(peers: ClusterPeer[], opts: ClusterMapOptions = {}): ClusterMapModel {
  const meshOn = opts.meshOn ?? true;
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
      reported: opts.reported?.has(p.address) ?? false,
      x: center.x + radius * Math.cos(angle),
      y: center.y + radius * Math.sin(angle),
    };
  });
  const edges = meshOn
    ? sorted
        .filter((p) => p.online)
        .map((p) => ({
          to: p.address,
          style: (p.link === "direct" ? "solid" : p.link === "relayed" ? "dashed" : "unknown") as EdgeStyle,
        }))
    : [];
  return {
    center,
    radius,
    nodes,
    edges,
    summary: meshOn ? summarize(sorted.length, edges) : summarizeOff(sorted.length),
    sentence: sentenceOf(nodes, edges, meshOn),
  };
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

function summarizeOff(total: number): string {
  if (total === 0) return "No other machines in this group yet.";
  return "The mesh is off on this device, so it has no live connections.";
}

function sentenceOf(nodes: MapNode[], edges: { style: EdgeStyle }[], meshOn: boolean): string {
  const devices = nodes.length + 1; // this machine is always on the map
  const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
  if (nodes.length === 0) return "Cluster map: 1 device, this machine, no peers yet.";
  const direct = edges.filter((e) => e.style === "solid").length;
  const relayed = edges.filter((e) => e.style === "dashed").length;
  const unknown = edges.filter((e) => e.style === "unknown").length;
  const notReported = nodes.filter((n) => !n.reported).length;
  const parts = [
    plural(devices, "device", "devices"),
    meshOn ? plural(direct, "direct link", "direct links") : "mesh off, no live links",
    ...(meshOn ? [`${relayed} relayed`] : []),
    ...(meshOn && unknown ? [`${unknown} with the path not reported`] : []),
    `${notReported} not reported`,
  ];
  return `Cluster map: ${parts.join(", ")}.`;
}
