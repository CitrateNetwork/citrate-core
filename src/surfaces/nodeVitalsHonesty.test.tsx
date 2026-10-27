// Q-A.4b — Node vitals display-honesty in the PACKAGED (tauri) build. The log
// panel is real (Q-A.2); the remaining vitals were fabricated or dead in tauri:
//   1. blocksProposed / cpu / ram were never folded from a real read and the sim
//      tick() fabricated them → in tauri they must show an honest "—"/label, never
//      a fabricated number.
//   2. K3 (contract §1): no sync percentage at all; "Syncing · N blocks behind" from read
//      numbers, or "Sync unknown" when the public head can't be read.
//   3. peer ROWS were never read (the sim fabricated them) → K3 shows the real local
//      peer COUNT ("N direct peers") and no per-peer rows; the field is gone.
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

import { renderStatus } from "./node/nodePage.fixture";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

// Node only touches `store` in effects/handlers (never fired by a static render)
// and refreshEarnings in an effect. A no-op stub covers render — EXCEPT for the
// predicates the markup calls directly during render (walletIsLinked gates the
// activate section), which must be present or the render throws.
const noopStore = { walletIsLinked: () => true } as unknown as Store;

// A running, validating node with the seeded sim blocksProposed in state. It must NOT surface as a
// number. K3: cpu / ram / peer rows are no longer state at all (no read exists for them).
function runningState(over: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  s.node = "validating";
  s.peers = 7;
  s.height = 510_000;
  s.syncPct = 100;
  s.blocksProposed = 412;
  s.hasGrant = true;
  return { ...s, ...over };
}

describe("Node vitals — Q-A.4b tauri display honesty", () => {
  it("blocksProposed shows an honest 'not a registered validator' state, not a fabricated number", () => {
    const html = renderStatus(noopStore, runningState());
    // NEGATIVE CONTROL — the fabricated seeded blocksProposed (412) must NOT render.
    expect(html).not.toContain("412");
    // POSITIVE — honest label (no real validator registration yet, Q-C).
    expect(html.toLowerCase()).toContain("not a registered validator");
  });

  it("cpu / ram do NOT show a fabricated number in tauri", () => {
    const html = renderStatus(noopStore, runningState());
    // K3: no CPU / memory / data-dir rows at all (no process read is wired), and not the sim's
    // hard-coded "3.1 GB" data dir or an unread "ON · keyring" claim.
    expect(html).not.toMatch(/>CPU<|>Memory<|>Data dir<|3\.1 GB|ON · keyring/);
    expect("cpu" in freshState("p1") || "ram" in freshState("p1")).toBe(false);
  });

  // K3 (contract §1): a syncing node reads "Syncing · N blocks behind" from the public head minus
  // the local height. No percentage is shown, real or not.
  it("syncing state shows the blocks behind from read numbers, never a percent", () => {
    const s = runningState({ node: "syncing", syncPct: 62, height: 100_000 });
    const html = renderStatus(noopStore, s, { publicHead: 100_120 });
    expect(html).toContain("Syncing · 120 blocks behind");
    expect(html).not.toMatch(/\d+%/);
    expect(html).not.toContain(">syncing…<");
  });

  it("an UNKNOWN sync percent renders '—', never a fabricated 100", () => {
    // The public head can't be read: Sync unknown, the head row "—", and no percent at all.
    const s = runningState({ node: "syncing", syncPct: -1 });
    const html = renderStatus(noopStore, s, { publicHead: null, publicHeadAgeMs: null });
    expect(html).toContain("Sync unknown · network head unavailable");
    expect(html).toContain("— · network head unavailable");
    expect(html).not.toMatch(/-?\d+%/);
  });

  it("peers show the REAL local count as direct peers, never fabricated peer rows", () => {
    const html = renderStatus(noopStore, runningState());
    // NEGATIVE CONTROL: no per-peer id / latency rows (none is read; the sim's are gone with peerRows).
    expect(html).not.toMatch(/16Uiu2HA|\d+ ms</);
    expect("peerRows" in freshState("p1")).toBe(false);
    // POSITIVE: the real local net_peerCount, worded as direct peers (contract §2), and why no detail.
    expect(html).toContain("7 direct peers");
    expect(html).toContain("Per-peer detail has no read yet");
  });

  it("no functional Pause/Resume control paints a 'paused' state (cosmetic buttons removed)", () => {
    const html = renderStatus(noopStore, runningState());
    // The cosmetic Pause/Resume buttons are gone — Stop remains (real supervisor).
    expect(html).not.toContain(">Pause<");
    expect(html).not.toContain(">Resume<");
    expect(html).toContain(">Stop node<");
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
    const html = renderStatus(noopStore, s);
    expect(html).not.toContain("not a registered validator");
    // C1-V V7: the row is "Your stake" = ValidatorRegistry.stakeOf (the old "bonded" wording went).
    expect(html).toContain("32,000 SALT");
  });

  it("an UNBONDED member is still honestly told so", () => {
    const s = runningState({ node: "synced", bondedStake: 0 });
    const html = renderStatus(noopStore, s);
    expect(html).toContain("not a registered validator");
  });

  it("blocks proposed stays '—' even when registered — there is no source for it", () => {
    const s = runningState({ node: "validating", bondedStake: 32000, blocksProposed: 7 });
    const html = renderStatus(noopStore, s);
    // NEGATIVE CONTROL: the seeded sim count must not surface as a real tally.
    expect(html).not.toContain(">7<");
  });
});
