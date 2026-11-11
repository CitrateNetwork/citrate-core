// V60 MESH-4: how a peer's live state reads in the Cluster screen, from the daemon's `link`.
import { describe, expect, it } from "vitest";
import { peerStateLabel } from "./clusterReachability";

describe("peerStateLabel", () => {
  it("says how an online peer is reached when the daemon says", () => {
    expect(peerStateLabel({ address: "aa", online: true, link: "direct" })).toBe("online · direct");
    expect(peerStateLabel({ address: "aa", online: true, link: "relayed" })).toBe("online · through a relay");
  });
  it("says only online when the daemon does not report a path (older daemon)", () => {
    expect(peerStateLabel({ address: "aa", online: true })).toBe("online");
  });
  it("never shows a path for an offline peer", () => {
    expect(peerStateLabel({ address: "aa", online: false })).toBe("authorized · offline");
    expect(peerStateLabel({ address: "aa", online: false, link: "relayed" })).toBe("authorized · offline");
  });
});
