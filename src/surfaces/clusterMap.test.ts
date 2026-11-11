// V60 MESH-4: the cluster map (03 §UX "Mesh"): devices as nodes, an edge per live connection,
// solid = direct, dashed = relayed. Source: the cluster daemon's `peers` answer (`cluster_peers`).
import { describe, expect, it } from "vitest";
import { clusterMapModel } from "./clusterMap";

const peer = (address: string, online: boolean, link?: "direct" | "relayed", member?: string) => ({
  address,
  online,
  ...(link ? { link } : {}),
  ...(member ? { member } : {}),
});

describe("clusterMapModel", () => {
  it("is just this machine when the roster is empty", () => {
    const m = clusterMapModel([]);
    expect(m.nodes).toEqual([]);
    expect(m.edges).toEqual([]);
    expect(m.summary).toBe("No other machines in this group yet.");
  });

  it("draws an edge only for an online peer, styled by how it is reached", () => {
    const m = clusterMapModel([
      peer("cc", false),
      peer("aa", true, "direct"),
      peer("bb", true, "relayed"),
      peer("dd", true),
    ]);
    expect(m.nodes.map((n) => n.address)).toEqual(["aa", "bb", "cc", "dd"]);
    expect(m.edges).toEqual([
      { to: "aa", style: "solid" },
      { to: "bb", style: "dashed" },
      { to: "dd", style: "unknown" },
    ]);
    expect(m.summary).toBe("3 of 4 online: 1 direct, 1 through a relay, 1 path not reported.");
  });

  it("places nodes on a circle around this machine, the same every time", () => {
    const peers = [peer("aa", true, "direct"), peer("bb", false), peer("cc", true, "relayed")];
    const a = clusterMapModel(peers);
    const b = clusterMapModel([...peers].reverse());
    expect(a.nodes).toEqual(b.nodes);
    for (const n of a.nodes) {
      const r = Math.hypot(n.x - a.center.x, n.y - a.center.y);
      expect(Math.abs(r - a.radius)).toBeLessThan(1e-6);
    }
    expect(new Set(a.nodes.map((n) => `${n.x.toFixed(3)},${n.y.toFixed(3)}`)).size).toBe(3);
  });

  it("labels a linked device by its member", () => {
    const m = clusterMapModel([peer("d1", true, "direct", "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1")]);
    expect(m.nodes[0].member).toBe("a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1");
  });
});
