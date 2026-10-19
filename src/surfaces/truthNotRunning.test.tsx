// C1-V (UI truth contract v1, citrate-chain docs/ui-truth/UI_TRUTH_CONTRACT.md) — the
// "not running" items. On 40204 today finality is not running (the finalized tag returns
// genesis), pin bonds are dormant, and PatronageLedger / FederatedRoundLedger are not
// deployed. The app must not draw any of them as working (rule 3), must not show a seeded
// or stale value as live (rule 2), and must show "—" plus a reason when it has no reading.
//
//   V1  finality described as running (Dashboard tile tip, coach, demo agent)
//   V2  heartbeat "last ack 4s ago" stuck at its seed value, sold as slashing protection
//   V3  pin projections (bonded x 0.0075 "per month") and "challenges passed" (pins x 9 + 4)
//   V10 seeded height 131,204 and persisted node state shown before the first poll
//   V11 Train reads PatronageLedger, which is not in the 40204 address book
import { describe, it, expect, vi, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { createRoot } from "react-dom/client";
import { act } from "react";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));

// Dashboard reads the public RPC through wagmi hooks; the static render only needs their
// return shapes. The live height is "pending" so the vitals fall back to the local node.
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
// The right rail (widgets, daemons, chat) is not under test here.
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Dashboard } from "./Dashboard";
import { Node } from "./Node";
import { Train, groupRoundsLive } from "./Train";
import { COACH_STEPS, NODE_LOG_TEMPLATES } from "../data/seed";
import { createDemoProvider, type AgentContext, type ToolCall } from "../agent/harness";
import { freshState, loadState, PERSIST_KEYS, STORAGE_KEY, type AppState } from "../shell/state";
import { store, type Store } from "../shell/store";
import { bridge } from "../bridge";
import book from "../../src-tauri/addresses/40204.json";

const stub = { walletIsLinked: () => true, refreshEarnings: () => {}, toast: () => {}, identity: () => ({ wallet: "" }) } as unknown as Store;

function running(over: Partial<AppState> = {}): AppState {
  return { ...freshState("p1"), node: "synced", peers: 3, syncPct: 100, height: 499_058, ...over };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("V1 — finality is not drawn as running", () => {
  it("the Dashboard finality tile says it is not running, with no checkpoint cadence", () => {
    const html = renderToStaticMarkup(<Dashboard store={stub} s={running()} />);
    expect(html).not.toContain("BFT checkpoint every");
    expect(html).not.toContain("checkpoint age");
    expect(html).toContain("not running yet");
  });

  it("the first-run coach does not describe finality or earnings as live", () => {
    const text = COACH_STEPS.map((c) => c.title + " " + c.body).join(" ").toLowerCase();
    expect(text).not.toContain("finality");
    expect(text).not.toContain("earns");
    expect(text).not.toContain("earnings");
  });

  it("the sim log tail has no finality vote, heartbeat ack or pin challenge lines", () => {
    const lines = NODE_LOG_TEMPLATES.join("\n");
    expect(lines).not.toMatch(/finality|checkpoint vote/);
    expect(lines).not.toMatch(/heartbeat/);
    expect(lines).not.toMatch(/pin: challenge/);
  });

  it("the model context carries no finality age", () => {
    expect(Object.keys(store.snapshot())).not.toContain("finalityAge");
  });

  it("the built-in agent's network reply says finality is not running, never that a checkpoint settled", async () => {
    vi.useFakeTimers();
    const ctx: AgentContext = { height: 499_058, peers: 3, nodeState: "synced", staked: 0, liquid: 0, claimable: 0, earningsToday: 0, walletAddr: "0xabc", tier: "pilot" };
    let text = "";
    const done = createDemoProvider(() => ctx).send({
      messages: [{ role: "user", content: "what's the network status?" }],
      callbacks: { onStatus: () => {}, onToken: (t: string) => (text += t), onToolCall: async (_c: ToolCall) => "ok" },
    });
    await vi.runAllTimersAsync();
    await done;
    expect(text).not.toMatch(/BFT|checkpoint settled|checkpoints land/i);
    expect(text.toLowerCase()).toContain("finality is not running");
    expect(text).toContain("499,058");
  });
});

describe("V2 — heartbeat has no source, so it shows unavailable", () => {
  it("never renders a seeded ack age or a slashing-protection claim", () => {
    const html = renderToStaticMarkup(<Node store={stub} s={running({ nTab: "earn" })} />);
    expect(html).not.toMatch(/last ack \d+s ago/);
    expect(html).not.toContain("slashing protection");
    expect(html).not.toContain("NematocystSlashing");
    expect(html).toContain("Heartbeat");
    expect(html).toContain("no heartbeat reading");
  });

  it("is not part of app state (nothing to go stale)", () => {
    expect("hb" in freshState("p1")).toBe(false);
  });
});

describe("V3 — pin bonds are dormant", () => {
  const pins = [
    { cid: "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi", bond: 120, cadH: 6, nextIn: 400, last: "ok" },
  ] as unknown as AppState["pins"];

  it("shows 'Pin bonds not live yet' and no projection or challenge tallies", () => {
    const html = renderToStaticMarkup(<Node store={stub} s={running({ nTab: "pin", pins })} />);
    expect(html).toContain("Pin bonds not live yet");
    expect(html).not.toContain("Projected");
    expect(html).not.toContain("at current demand");
    expect(html).not.toMatch(/passed · 0 failed/);
    // 1 pin x 9 + 4 = 13 was the made-up "challenges passed" figure.
    expect(html).not.toContain(">13<");
    expect(html).not.toContain("Plan &amp; sign");
  });
});

describe("V10 — no seeded height or persisted node state before the first poll", () => {
  it("a fresh state has no height reading (0 = not read), not a seeded 131,2xx", () => {
    const s = freshState("p1");
    expect(s.height).toBe(0);
  });

  it("the Dashboard shows '—' for the local node height until the node reports one", () => {
    const html = renderToStaticMarkup(<Dashboard store={stub} s={running({ height: 0 })} />);
    expect(html).not.toMatch(/>131,2\d\d</);
    expect(html).toContain("not read yet");
  });

  it("node state, sync percent and peers are not persisted", () => {
    expect(PERSIST_KEYS).not.toContain("node");
    expect(PERSIST_KEYS).not.toContain("syncPct");
    expect(PERSIST_KEYS).not.toContain("peers");
  });

  it("a state saved by an older build does not bring back the last session's node state", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ persona: "p1", node: "validating", syncPct: 100, peers: 9 }));
    try {
      const s = loadState();
      expect(s.node).toBe("off");
      expect(s.syncPct).toBe(0);
      expect(s.peers).toBe(0);
    } finally {
      localStorage.removeItem(STORAGE_KEY);
    }
  });
});

describe("V11 — group rounds are not live on 40204", () => {
  it("the shipped address book has no PatronageLedger, so rounds read as not live", () => {
    expect((book.addresses as Record<string, string>).PatronageLedger).toBeUndefined();
    expect(groupRoundsLive(book.addresses)).toBe(false);
    expect(groupRoundsLive({ PatronageLedger: "0x" + "0".repeat(40) })).toBe(false);
    expect(groupRoundsLive({ PatronageLedger: "0x00000000000000000000000000000000000000AB", FederatedRoundLedger: "0x00000000000000000000000000000000000000CD" })).toBe(true);
  });

  it("Train shows 'Group rounds not live yet' and never reads the ledger", async () => {
    vi.spyOn(bridge.groups, "list").mockResolvedValue([{ id: "0x" + "ab".repeat(32), name: "Study group" }] as never);
    const status = vi.spyOn(bridge.training, "status");
    vi.spyOn(bridge.flRounds, "overview").mockRejectedValue(new Error("not configured"));
    const el = document.createElement("div");
    const root = createRoot(el);
    await act(async () => {
      root.render(<Train store={stub} s={freshState("p1")} />);
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(el.textContent).toContain("Group rounds not live yet");
    expect(el.textContent).not.toContain("read live from 40204 (PatronageLedger)");
    expect(status).not.toHaveBeenCalled();
    act(() => root.unmount());
  });
});
