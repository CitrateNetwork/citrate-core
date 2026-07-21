// =====================================================================
// citrate-core — memory-graph layout (Q-A.4a). Pure, deterministic layout of the
// REAL mem-mcp store nodes into the 2.5D constellation coordinates. Extracted from
// Storage.tsx so the Store can build the graph from a `bridge.memory.*` read
// (constellation / search) and fold it into AppState — the Storage surface renders
// whatever the Store holds. NO RNG (re-fetching never jitters the layout) and NO
// fabricated nodes: it lays out exactly the nodes the daemon returned (Rule 1).
// =====================================================================
import type { MemGraph, MemGraphNode } from "./state";

/**
 * Deterministic layout for the REAL store nodes: personal nodes on the left,
 * chain-state on the right, each ringed by tenant so the same store always lays
 * out identically (a pure function of the node ids + order — no RNG).
 */
export function layoutGraph(
  tenants: { tenant: string; totalInTenant: number }[],
  rawNodes: { id: string; label: string; tenant: string; kind: string }[],
): MemGraph {
  const centers: Record<string, { cx: number; cy: number }> = {
    personal: { cx: 430, cy: 280 },
    "chain-state": { cx: 830, cy: 280 },
    "chain-facts": { cx: 830, cy: 280 },
  };
  const perTenant: Record<string, number> = {};
  const nodes: MemGraphNode[] = rawNodes.map((n, i) => {
    const c = centers[n.tenant] ?? { cx: 630, cy: 280 };
    const k = (perTenant[n.tenant] = (perTenant[n.tenant] ?? 0) + 1) - 1;
    // Ring placement: golden-angle-ish deterministic spiral for a calm spread.
    const ang = k * 2.399963; // golden angle in radians
    const rad = 34 + k * 15;
    return {
      id: n.id,
      label: n.label,
      tenant: n.tenant,
      kind: n.kind,
      detail: `${n.kind} · tenant ${n.tenant}`,
      x: c.cx + Math.cos(ang) * rad,
      y: c.cy + Math.sin(ang) * rad,
      z: 0.85 + (i % 5) * 0.12,
    };
  });
  // Links: connect each node to the previous node in its tenant (a stable spine).
  const byTenant: Record<string, string[]> = {};
  nodes.forEach((n) => (byTenant[n.tenant] = byTenant[n.tenant] ?? []).push(n.id));
  const links: [string, string][] = [];
  Object.values(byTenant).forEach((ids) => {
    for (let i = 1; i < ids.length; i++) links.push([ids[i - 1], ids[i]]);
  });
  return { nodes, links, tenants };
}
