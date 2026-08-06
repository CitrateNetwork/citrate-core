// Q-A.4b — Node vitals display-honesty in the PACKAGED (tauri) build. The log
// panel is real (Q-A.2); the remaining vitals were fabricated or dead in tauri:
//   1. blocksProposed / cpu / ram were never folded from a real read and the sim
//      tick() fabricated them → in tauri they must show an honest "—"/label, never
//      a fabricated number.
//   2. syncPct is a binary 0/100 stub (node.rs) → the UI must not imply a precise
//      fake percent; it renders "syncing…"/"synced" honestly.
//   3. peer ROWS are dead in tauri (never folded; sim makePeers() fabricated) →
//      show the real peer COUNT + an honest "peer detail coming" empty state, never
//      fabricated peer rows.
//   4. Pause/Resume were cosmetic (setState only, no supervisor call) → they must
//      not exist as functional-looking controls that paint a "paused" label.
//
// These render Node in TAURI mode (mocked) and assert the honest states + the
// negative controls (a fabricated number/peer row must NOT appear).
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

// Force packaged-app mode for this whole file.
vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));

import { Node } from "./Node";
import { freshState, type AppState, type PeerRow } from "../shell/state";
import type { Store } from "../shell/store";

// Node only touches `store` in effects/handlers (never fired by a static render)
// and refreshEarnings in an effect. A no-op stub covers render — EXCEPT for the
// predicates the markup calls directly during render (walletIsLinked gates the
// activate section), which must be present or the render throws.
const noopStore = { walletIsLinked: () => true } as unknown as Store;

// A running, validating node with the fabricated sim vitals PRE-SEEDED into state
// (blocksProposed / cpu / ram / peerRows). In tauri these must NOT surface as
// numbers/rows — the fix makes them honest regardless of a stale seeded value.
function runningState(over: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  s.node = "validating";
  s.peers = 7;
  s.syncPct = 100;
  s.blocksProposed = 412;
  s.cpu = 23;
  s.ram = 780;
  s.hasGrant = true;
  s.selfStake = 500;
  return { ...s, ...over };
}

describe("Node vitals — Q-A.4b tauri display honesty", () => {
  it("blocksProposed shows an honest 'not a registered validator' state, not a fabricated number", () => {
    const html = renderToStaticMarkup(<Node store={noopStore} s={runningState()} />);
    // NEGATIVE CONTROL — the fabricated seeded blocksProposed (412) must NOT render.
    expect(html).not.toContain("412");
    // POSITIVE — honest label (no real validator registration yet, Q-C).
    expect(html.toLowerCase()).toContain("not a registered validator");
  });

  it("cpu / ram do NOT show a fabricated number in tauri", () => {
    const html = renderToStaticMarkup(<Node store={noopStore} s={runningState()} />);
    // NEGATIVE CONTROL — the fabricated seeded cpu (23 %) / ram (780 MB) must NOT render.
    expect(html).not.toContain("23 %");
    expect(html).not.toContain("780 MB");
  });

  // syncPct used to be a 0/100 stub, so this asserted a bare "syncing…". It is now
  // a REAL percentage (local head vs the authoritative network tip, node.rs), so
  // the honest thing is to SHOW it — and to show "—" when the tip is unknown.
  it("syncing state shows the REAL percent", () => {
    const s = runningState({ node: "syncing", syncPct: 62 });
    const html = renderToStaticMarkup(<Node store={noopStore} s={s} />);
    expect(html).toContain(">62%<");
    // The old stub wording must not come back — it hid real progress.
    expect(html).not.toContain(">syncing…<");
  });

  it("an UNKNOWN sync percent renders '—', never a fabricated 100", () => {
    // node.rs yields a negative syncPct when the network tip cannot be read.
    const s = runningState({ node: "syncing", syncPct: -1 });
    const html = renderToStaticMarkup(<Node store={noopStore} s={s} />);
    expect(html).toContain(">—<");
    expect(html).not.toMatch(/>-?\d+%</); // no "-1%", and no invented "100%"
  });

  it("peers panel shows the REAL count + an honest 'peer detail coming', never fabricated peer rows", () => {
    // Seed fabricated per-peer rows; in tauri they must NOT render.
    const fakeRows: PeerRow[] = [
      { id: "16Uiu2HAdeadbeef…fra1", dir: "in", lat: "42 ms" },
      { id: "16Uiu2HAcafef00d…sgp1", dir: "out", lat: "88 ms" },
    ];
    const html = renderToStaticMarkup(<Node store={noopStore} s={runningState({ peerRows: fakeRows })} />);
    // NEGATIVE CONTROL — no fabricated peer-id / latency row.
    expect(html).not.toContain("16Uiu2HAdeadbeef");
    expect(html).not.toContain("42 ms");
    // POSITIVE — the real peer count (7) is shown, plus an honest empty state.
    expect(html).toContain("7");
    expect(html.toLowerCase()).toContain("peer detail coming");
  });

  it("no functional Pause/Resume control paints a 'paused' state (cosmetic buttons removed)", () => {
    const html = renderToStaticMarkup(<Node store={noopStore} s={runningState()} />);
    // The cosmetic Pause/Resume buttons are gone — Stop remains (real supervisor).
    expect(html).not.toContain(">Pause<");
    expect(html).not.toContain(">Resume<");
    expect(html).toContain(">Stop<");
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// VALIDATOR REGISTRATION must be READ, not asserted. `blocksProposedStr` was the
// literal "not a registered validator" for EVERY tauri build, so a member who had
// just bonded 32,000 SALT — clone deployed, pubkeyOfStaker set, producing blocks —
// was told they were not a validator (observed 2026-08-06 immediately after a
// successful MemberBond.activate).
// ─────────────────────────────────────────────────────────────────────────────

describe("Node — validator registration reflects the real bond", () => {
  it("a BONDED validator is not told it is unregistered", () => {
    const s = runningState({ node: "validating", bondedStake: 32000 });
    const html = renderToStaticMarkup(<Node store={noopStore} s={s} />);
    expect(html).not.toContain("not a registered validator");
    expect(html).toContain("32,000 SALT bonded");
  });

  it("an UNBONDED member is still honestly told so", () => {
    const s = runningState({ node: "synced", bondedStake: 0 });
    const html = renderToStaticMarkup(<Node store={noopStore} s={s} />);
    expect(html).toContain("not a registered validator");
  });

  it("blocks proposed stays '—' even when registered — there is no source for it", () => {
    const s = runningState({ node: "validating", bondedStake: 32000, blocksProposed: 7 });
    const html = renderToStaticMarkup(<Node store={noopStore} s={s} />);
    // NEGATIVE CONTROL: the seeded sim count must not surface as a real tally.
    expect(html).not.toContain(">7<");
  });
});
