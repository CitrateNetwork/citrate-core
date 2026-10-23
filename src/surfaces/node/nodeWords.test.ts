// K3 — Node › Status words (UI truth contract §1 "Network and node"). Unknown is never zero, nothing
// not-live is shown as running, and the words are exactly the contract's.
import { describe, expect, it } from "vitest";
import { activationOffer, directPeers, heightLine, nodeWord, statusSentence, type NodeWordInput } from "./nodeWords";

const base: NodeWordInput = {
  node: "synced",
  blocked: null,
  compat: "compatible",
  localHeight: 510_030,
  publicHead: 510_032,
  publicHeadAgeMs: 2_000,
  publicHeadError: false,
  peers: 4,
  peersZeroForMs: 0,
  localStillForMs: 0,
};
const w = (p: Partial<NodeWordInput>) => nodeWord({ ...base, ...p }).word;

describe("node word (contract §1)", () => {
  it("Off when the process isn't running, Starting before its RPC answers", () => {
    expect(w({ node: "off", localHeight: 0 })).toBe("Off");
    expect(w({ node: "paused" })).toBe("Off");
    expect(w({ node: "prov", localHeight: 0 })).toBe("Starting");
    // A running supervisor whose RPC has not answered (height 0x0) is Starting, never block 0.
    expect(w({ node: "syncing", localHeight: 0 })).toBe("Starting");
  });

  it("Synced only within 5 blocks of the public head", () => {
    expect(w({ localHeight: 510_027, publicHead: 510_032 })).toBe("Synced");
    expect(w({ localHeight: 510_026, publicHead: 510_032 })).toBe("Syncing · 6 blocks behind");
    // The store's 'validating' (stake >= 32k) is not a node word: the node word is still Synced.
    expect(w({ node: "validating" })).toBe("Synced");
  });

  it("Syncing · N blocks behind uses read numbers", () => {
    expect(w({ node: "syncing", localHeight: 100_000, publicHead: 510_000 })).toBe("Syncing · 410,000 blocks behind");
  });

  it("Sync unknown · network head unavailable when the public head can't be read (contract v1.1 §1)", () => {
    const U = "Sync unknown · network head unavailable";
    expect(w({ publicHead: null, publicHeadAgeMs: null })).toBe(U);
    expect(w({ node: "syncing", localHeight: 100_000, publicHead: null, publicHeadAgeMs: null })).toBe(U);
    // 0x0 is the error value of eth_blockNumber, never a head.
    expect(w({ publicHead: 0 })).toBe(U);
    // An errored poll keeps wagmi's last data: it is not a current head.
    expect(w({ publicHeadError: true })).toBe(U);
    // A last good read older than 60 s is stale: never "Synced" against it.
    expect(w({ publicHeadAgeMs: 60_001 })).toBe(U);
    expect(w({ publicHeadAgeMs: 60_000 })).toBe("Synced");
    // A head read with no known age is not fresh either.
    expect(w({ publicHeadAgeMs: null })).toBe(U);
  });

  it("Sync stalled when the local height hasn't advanced for 60 s with peers > 0", () => {
    expect(w({ localStillForMs: 59_000 })).toBe("Synced");
    expect(w({ localStillForMs: 60_000 })).toBe("Sync stalled");
    expect(w({ node: "syncing", localHeight: 100_000, publicHead: 510_000, localStillForMs: 61_000 })).toBe("Sync stalled");
    // With no peers it is a connectivity problem, not a stall.
    expect(w({ peers: 0, peersZeroForMs: 61_000, localStillForMs: 61_000 })).toBe("No peers");
    // A frozen height also beats an unreadable head: the freeze is a local, fresh fact.
    expect(w({ publicHead: null, publicHeadAgeMs: null, localStillForMs: 61_000 })).toBe("Sync stalled");
  });

  it("Sync stalled from the store's 60 s freeze; No peers only after 60 s at zero", () => {
    expect(w({ node: "stalled" })).toBe("Sync stalled");
    expect(w({ peers: 0, peersZeroForMs: 59_000 })).toBe("Synced");
    expect(w({ peers: 0, peersZeroForMs: 60_000 })).toBe("No peers");
  });

  it("Update required wins over No peers when the genesis check fails", () => {
    expect(w({ compat: "retired", peers: 0, peersZeroForMs: 120_000 })).toBe("Update required");
    // An unknown genesis check is not a failure.
    expect(w({ compat: "unknown" })).toBe("Synced");
  });

  it("Error for a failed supervisor or a refused start, with the holder named", () => {
    expect(w({ node: "error" })).toBe("Error");
    const blocked = nodeWord({ ...base, node: "off", blocked: "Another process (pid 4312) holds the chain database." });
    expect(blocked.word).toBe("Error");
    expect(blocked.why).toContain("pid 4312");
  });

  it("never uses a banned word", () => {
    const inputs: Partial<NodeWordInput>[] = [
      {},
      { node: "validating" },
      { node: "syncing", localHeight: 5 },
      { node: "stalled" },
      { compat: "retired" },
      { node: "error" },
      { node: "off" },
    ];
    for (const p of inputs) {
      const r = nodeWord({ ...base, ...p });
      expect(r.word + " " + r.why).not.toMatch(/final|validating|producing|earning|%/i);
    }
  });
});

describe("status sentence and rows", () => {
  it("is the node word, plus the validator word only when it was read", () => {
    const n = nodeWord(base);
    expect(statusSentence(n, null)).toBe("Synced");
    expect(statusSentence(n, "Validator active")).toBe("Synced · Validator active");
  });

  it("direct peers are unknown while the node isn't answering, never 0", () => {
    expect(directPeers(false, 0, 0)).toBe("— · not read");
    expect(directPeers(true, 0, 0)).toBe("— · not read");
    expect(directPeers(true, 10, 1)).toBe("1 direct peer");
    expect(directPeers(true, 10, 3)).toBe("3 direct peers");
  });

  it("height is '—' before a reading and names the gap to the public head", () => {
    expect(heightLine(0, 510_000)).toBe("—");
    expect(heightLine(509_990, 510_000)).toBe("509,990 · 10 behind the public head");
    expect(heightLine(510_000, 510_000)).toBe("510,000 · at the public head");
    expect(heightLine(510_000, null)).toBe("510,000 (this node) · network head unavailable");
  });
});

describe("Activate validator offer (#366 canOfferActivation, contract §1 Validators)", () => {
  const NONE = { isActive: false, status: 0, actEpoch: 0, currentEpoch: 40 };
  const LOCKED = { deployed: true, activated: false, pubkey: null, unlocked: false };
  const member = (over: Partial<Parameters<typeof activationOffer>[1]> = {}): Parameters<typeof activationOffer>[1] => ({
    node: "synced",
    bondedStake: 0,
    validatorActivation: NONE,
    hasGrant: true,
    memberBond: LOCKED,
    ...over,
  });
  it("offers only a locked, not activated, not staked bond on a Synced page", () => {
    expect(activationOffer("Synced", member())).toEqual({ offer: true, why: "" });
  });
  it("unread registry status or bond state: no Activate, '—' and the reason", () => {
    for (const s of [member({ validatorActivation: null, bondedStake: 32_000 }), member({ memberBond: null }), member({ memberBond: { deployed: true, activated: null, pubkey: null } })]) {
      const o = activationOffer("Synced", s);
      expect(o.offer).toBe(false);
      expect(o.why).toMatch(/^— · /);
    }
  });
  it("Exiting and Slashed say the word whatever the bond reads, and offer nothing", () => {
    for (const memberBond of [null, LOCKED, { deployed: true, activated: true, pubkey: "0x" + "ab".repeat(48) }]) {
      expect(activationOffer("Synced", member({ validatorActivation: { ...NONE, status: 2 }, memberBond })).why).toMatch(/^Exiting · /);
      expect(activationOffer("Synced", member({ validatorActivation: { ...NONE, status: 3 }, memberBond })).why).toMatch(/^Slashed · /);
    }
  });
  it("an activated bond or a registered key never offers Activate (it would revert AlreadyActivated)", () => {
    const activated = { deployed: true, activated: true, pubkey: "0x" + "ab".repeat(48), unlocked: false };
    expect(activationOffer("Synced", member({ memberBond: activated })).offer).toBe(false);
    const registered = activationOffer("Synced", member({ validatorActivation: { isActive: true, status: 1, actEpoch: 42, currentEpoch: 40 }, bondedStake: 32_000, memberBond: activated }));
    expect(registered).toMatchObject({ offer: false });
    expect(registered.why).toContain("already registered");
  });
  it("the activation window: unlocked is closed, unread fails closed", () => {
    expect(activationOffer("Synced", member({ memberBond: { ...LOCKED, unlocked: true } })).why).toContain("activation window closed");
    expect(activationOffer("Synced", member({ memberBond: { ...LOCKED, unlocked: null } })).why).toContain("bond lock not read yet");
  });
  it("Sync unknown, Syncing or a supervisor not reporting synced: not offered, and says so", () => {
    expect(activationOffer("Sync unknown · network head unavailable", member()).why).toContain("Activation opens once this node reads Synced");
    expect(activationOffer("Syncing · 70 blocks behind", member()).offer).toBe(false);
    expect(activationOffer("Synced", member({ node: "syncing" })).offer).toBe(false);
  });
});
