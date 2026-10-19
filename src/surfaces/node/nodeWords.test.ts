// K3 — Node › Status words (UI truth contract §1 "Network and node"). Unknown is never zero, nothing
// not-live is shown as running, and the words are exactly the contract's.
import { describe, expect, it } from "vitest";
import { directPeers, heightLine, nodeWord, statusSentence, type NodeWordInput } from "./nodeWords";

const base: NodeWordInput = {
  node: "synced",
  blocked: null,
  compat: "compatible",
  localHeight: 510_030,
  publicHead: 510_032,
  peers: 4,
  peersZeroForMs: 0,
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

  it("Syncing · N blocks behind uses read numbers; an unread public head is '—', never a guess", () => {
    expect(w({ node: "syncing", localHeight: 100_000, publicHead: 510_000 })).toBe("Syncing · 410,000 blocks behind");
    expect(w({ node: "syncing", localHeight: 100_000, publicHead: null })).toBe("Syncing · — blocks behind");
    expect(w({ node: "synced", publicHead: null })).toBe("Syncing · — blocks behind");
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
    expect(heightLine(510_000, null)).toBe("510,000 · public head not read");
  });
});
