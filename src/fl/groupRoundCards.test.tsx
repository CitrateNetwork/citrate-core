// FL round cards (0.5.2, first cut), flag ON. The cards read the round ledgers ONLY through the
// address book (src-tauri/addresses/40204.json). No address: "Group rounds not live yet" and no
// chain read at all. An address with code: an honest dormant state. A failed read: unknown, never
// a fabricated round. The design's round states exist only as labelled fixtures, here in tests.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { createRoot } from "react-dom/client";
import { act } from "react";
import { WagmiProvider } from "wagmi";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { wagmiConfig } from "../wagmi";
import { PlaceView } from "../shell/PlaceView";
import { store } from "../shell/store";
import { freshState } from "../shell/state";
import { Groups } from "../surfaces/Groups";
import { groupsSlice } from "../shell/slices/groups";
import type { Store } from "../shell/store";
import realBook from "../../src-tauri/addresses/40204.json";
import { flRoundCardsEnabled } from "./roundCardsFlag";
import { readGroupRounds, roundLedgerPins, type GroupRoundsReading } from "./groupRoundsLedger";
import { GroupRoundsCard, GroupRoundsView } from "./GroupRoundsCard";
import { FIXTURE_ROUND_STATES, FixtureRoundCard } from "./fixtures/groupRoundStates.fixture";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const FRL = "0x" + "a1".repeat(20);
const PL = "0x" + "b2".repeat(20);
// The real book with neither round ledger, plus `extra` (today's r1006 book pins PatronageLedger).
const withoutLedgers = (): Record<string, string> => {
  const a: Record<string, string> = { ...(realBook.addresses as Record<string, string>) };
  delete a.PatronageLedger;
  delete a.FederatedRoundLedger;
  return a;
};
const bookWith = (extra: Record<string, string>) => ({ addresses: { ...withoutLedgers(), ...extra } });

const BANNED = /earn|income|projected|estimate|reward|yield|apy|\bfinal(ized)?\b/i;

describe("the flag", () => {
  afterEach(() => vi.unstubAllEnvs());
  it("is OFF by default", () => {
    vi.stubEnv("VITE_FL_ROUND_CARDS", "");
    expect(flRoundCardsEnabled()).toBe(false);
  });
  it("is ON only for exactly 1", () => {
    for (const v of ["true", "0", "yes", " 1"]) {
      vi.stubEnv("VITE_FL_ROUND_CARDS", v);
      expect(flRoundCardsEnabled(), v).toBe(false);
    }
    vi.stubEnv("VITE_FL_ROUND_CARDS", "1");
    expect(flRoundCardsEnabled()).toBe(true);
  });
});

describe("address book pins", () => {
  it("today's book (r1006) pins PatronageLedger but not FederatedRoundLedger", () => {
    // r1006 deploys PatronageLedger in block 0 (canonical book, citrate-chain a857443e);
    // FederatedRoundLedger is not in the book, so rounds stay not live.
    expect(roundLedgerPins(realBook)).toEqual([
      { name: "FederatedRoundLedger", address: null },
      { name: "PatronageLedger", address: "0x41f68b9e710585b064459a0ec706148fbf503659" },
    ]);
  });
  it("a malformed or zero address is not a pin", () => {
    const pins = roundLedgerPins(bookWith({ FederatedRoundLedger: "0x1234", PatronageLedger: "0x" + "0".repeat(40) }));
    expect(pins.map((p) => p.address)).toEqual([null, null]);
  });
  it("reads both pins, lowercased", () => {
    const pins = roundLedgerPins(bookWith({ FederatedRoundLedger: FRL.toUpperCase().replace("0X", "0x"), PatronageLedger: PL }));
    expect(pins.map((p) => p.address)).toEqual([FRL, PL]);
  });
});

describe("readGroupRounds", () => {
  it("no address in the book: not live, and nothing is read from the chain", async () => {
    const codeSize = vi.fn();
    const r = await readGroupRounds(bookWith({}), codeSize);
    expect(r).toEqual({ kind: "not-live", missing: ["FederatedRoundLedger", "PatronageLedger"] });
    expect(codeSize).not.toHaveBeenCalled();
  });
  it("today's book (one ledger pinned): not live, and nothing is read from the chain", async () => {
    const codeSize = vi.fn();
    const r = await readGroupRounds(realBook, codeSize);
    expect(r).toEqual({ kind: "not-live", missing: ["FederatedRoundLedger"] });
    expect(codeSize).not.toHaveBeenCalled();
  });
  it("one ledger pinned is still not live, and still no read", async () => {
    const codeSize = vi.fn();
    const r = await readGroupRounds(bookWith({ FederatedRoundLedger: FRL }), codeSize);
    expect(r).toEqual({ kind: "not-live", missing: ["PatronageLedger"] });
    expect(codeSize).not.toHaveBeenCalled();
  });
  it("both pinned with code: dormant, with the code sizes read", async () => {
    const codeSize = vi.fn(async (a: string) => (a === FRL ? 9000 : 4000));
    const r = await readGroupRounds(bookWith({ FederatedRoundLedger: FRL, PatronageLedger: PL }), codeSize);
    expect(r).toEqual({
      kind: "dormant",
      ledgers: [
        { name: "FederatedRoundLedger", address: FRL, codeSize: 9000 },
        { name: "PatronageLedger", address: PL, codeSize: 4000 },
      ],
    });
    expect(codeSize.mock.calls.map((c) => c[0])).toEqual([FRL, PL]);
  });
  it("pinned but no code at the address: not live (no-code)", async () => {
    const r = await readGroupRounds(bookWith({ FederatedRoundLedger: FRL, PatronageLedger: PL }), async (a) => (a === PL ? 0 : 9000));
    expect(r).toEqual({ kind: "no-code", empty: ["PatronageLedger"] });
  });
  it("a failed read is unknown, never zero and never dormant", async () => {
    const r = await readGroupRounds(bookWith({ FederatedRoundLedger: FRL, PatronageLedger: PL }), async () => {
      throw new Error("rpc down");
    });
    expect(r).toEqual({ kind: "unavailable", reason: "rpc down" });
  });
});

describe("GroupRoundsView copy", () => {
  const states: GroupRoundsReading[] = [
    { kind: "loading" },
    { kind: "not-live", missing: ["FederatedRoundLedger", "PatronageLedger"] },
    { kind: "no-code", empty: ["PatronageLedger"] },
    { kind: "unavailable", reason: "rpc down" },
    { kind: "dormant", ledgers: [{ name: "FederatedRoundLedger", address: FRL, codeSize: 9000 }, { name: "PatronageLedger", address: PL, codeSize: 4000 }] },
  ];
  const view = (r: GroupRoundsReading, variant: "chat" | "tab" = "tab") => renderToStaticMarkup(<GroupRoundsView reading={r} variant={variant} />);

  it("not live and no-code use the required wording", () => {
    expect(view(states[1])).toContain("Group rounds not live yet");
    expect(view(states[1], "chat")).toContain("Group rounds not live yet");
    expect(view(states[2])).toContain("Group rounds not live yet");
    expect(view(states[2])).toContain("no contract code");
  });
  it("dormant says deployed and dormant, never open or running", () => {
    const h = view(states[4]);
    expect(h).toContain("Deployed · dormant");
    expect(h).not.toMatch(/round open|running|live round/i);
  });
  it("unavailable shows unknown with the reason, not a zero", () => {
    const h = view(states[3]);
    expect(h).toContain("—");
    expect(h).toContain("rpc down");
    expect(h).not.toMatch(/>0</);
  });
  it("names its data source in every state (Rule 7)", () => {
    for (const r of states) expect(view(r)).toContain("40204.json");
  });
  it("has no rewards projection or earning language, and no fixture label, in any production state", () => {
    for (const r of states) for (const v of ["chat", "tab"] as const) {
      const h = view(r, v);
      expect(h, `${r.kind}/${v}`).not.toMatch(BANNED);
      expect(h.toLowerCase(), `${r.kind}/${v}`).not.toContain("fixture");
    }
  });
});

describe("the design's round states are fixtures, and only in tests", () => {
  it("every fixture card is labelled fixture and has no earning language", () => {
    expect(FIXTURE_ROUND_STATES.length).toBeGreaterThanOrEqual(3);
    for (const f of FIXTURE_ROUND_STATES) {
      const h = renderToStaticMarkup(<FixtureRoundCard state={f} />);
      expect(h.toLowerCase()).toContain("fixture");
      expect(h, f.status).not.toMatch(BANNED);
    }
  });
  it("no production module imports the fixtures", () => {
    const hits: string[] = [];
    const walk = (dir: string) => {
      for (const n of readdirSync(dir)) {
        const p = join(dir, n);
        if (statSync(p).isDirectory()) walk(p);
        else if (/\.(ts|tsx)$/.test(n) && !/\.test\.(ts|tsx)$/.test(n) && !p.includes(`${join("fl", "fixtures")}`)) {
          if (readFileSync(p, "utf8").includes("groupRoundStates.fixture")) hits.push(p);
        }
      }
    };
    walk(join(__dirname, ".."));
    expect(hits).toEqual([]);
  });
});

describe("flag ON in the app", () => {
  beforeEach(() => vi.stubEnv("VITE_FL_ROUND_CARDS", "1"));
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.restoreAllMocks();
    groupsSlice.set({ groups: [], selectedId: null });
  });

  const rounds = () =>
    renderToStaticMarkup(
      <WagmiProvider config={wagmiConfig}>
        <QueryClientProvider client={new QueryClient()}>
          <PlaceView store={store} s={{ ...freshState("p1"), stage: "done", route: "groups/rounds" }} />
        </QueryClientProvider>
      </WagmiProvider>,
    );

  it("Groups › Rounds shows the card, not the PatronageLedger read (V11)", () => {
    const h = rounds();
    expect(h).toContain('data-testid="group-rounds-card"');
    expect(h).toContain("Group rounds not live yet");
    expect(h).not.toContain("Train together");
    expect(h).not.toContain("reads live");
  });

  it("a group's chat shows the round card above the conversation", () => {
    const stubStore = { toast: () => {}, identity: () => ({ wallet: "" }) } as unknown as Store;
    const GROUP = { id: "0x" + "ab".repeat(16), name: "Garden club", owner: "0x" + "11".repeat(20), kind: "channel" as const, members: [] };
    groupsSlice.set({ groups: [GROUP], selectedId: GROUP.id, roster: [], messages: [] });
    const h = renderToStaticMarkup(<Groups store={stubStore} s={freshState("p1")} />);
    expect(h).toContain('data-testid="group-rounds-card"');
    expect(h).toContain("Group rounds not live yet");
    expect(h.indexOf('data-testid="group-rounds-card"')).toBeLessThan(h.indexOf("Nothing here yet"));
  });

  it("mounted with today's book, the card never calls the chain", async () => {
    const codeSize = vi.fn(async () => 1);
    const el = document.createElement("div");
    const root = createRoot(el);
    await act(async () => root.render(<GroupRoundsCard variant="tab" codeSize={codeSize} />));
    expect(el.textContent).toContain("Group rounds not live yet");
    expect(codeSize).not.toHaveBeenCalled();
    act(() => root.unmount());
  });

  it("mounted with both pins, the card reads code size and shows dormant", async () => {
    const codeSize = vi.fn(async () => 1234);
    const el = document.createElement("div");
    const root = createRoot(el);
    await act(async () => root.render(<GroupRoundsCard variant="tab" codeSize={codeSize} book={bookWith({ FederatedRoundLedger: FRL, PatronageLedger: PL })} />));
    expect(codeSize).toHaveBeenCalledTimes(2);
    expect(el.textContent).toContain("Deployed · dormant");
    act(() => root.unmount());
  });
});
