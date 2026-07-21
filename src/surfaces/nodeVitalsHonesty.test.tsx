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
// and refreshEarnings in an effect. A no-op stub covers render.
const noopStore = {} as unknown as Store;

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

  it("syncing state shows 'syncing…' not a precise fake percent (binary 0/100 stub)", () => {
    const s = runningState({ node: "syncing", syncPct: 0 });
    const html = renderToStaticMarkup(<Node store={noopStore} s={s} />);
    // No precise-looking fake percent label from the binary stub. The visible
    // sync readout must read "syncing…", not ">0%<"/">2%<" etc. (the bar's CSS
    // width:100% is not a user-facing percent value).
    expect(html).toContain(">syncing…<");
    expect(html).not.toMatch(/>\d+%</);
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
