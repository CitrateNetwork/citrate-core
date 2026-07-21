// Q-A.4b — Wallet display-honesty in the PACKAGED (tauri) build:
//   5. Paymaster bar (Overview) is a fabricated `sponsorUnits` with no chain
//      source (40204 has no paymaster read) → HIDDEN in tauri (never "X of 5
//      units" to a packaged user).
//   6. Staking "rewards accrued" (earnVal+earnPin+earnComp) is sim-seeded with no
//      real per-source chain read in tauri → show "—", never a fabricated accrued
//      number.
//   7. The Staked card hardcoded "32000" on top of real selfStake → render the
//      REAL attributed stake (s5StakeWei, wei→SALT) instead of the literal 32,000.
//
// These render Wallet in TAURI mode (mocked) and assert the honest states + the
// negative controls (a fabricated sponsor bar / accrued number must NOT appear).
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

// Force packaged-app mode for this whole file.
vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));

import { Wallet } from "./Wallet";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

// Wallet calls store.identity() at render + refreshes in effects (never fired by a
// static render). A tiny stub covers the render-time call.
const stubStore = {
  identity: () => ({
    name: "Member",
    initials: "M",
    email: "member@example.com",
    sub: "sub-1",
    wallet: "0x" + "0".repeat(40),
    tier: "pilot",
    role: "member",
    org: null,
    real: true,
  }),
} as unknown as Store;

function walletState(tab: string, over: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  s.wTab = tab;
  s.signedIn = true;
  // A real granted member with a real attributed stake read (33,500 SALT — a
  // DELIBERATELY non-32,000 value so a hardcoded 32,000 is detectable).
  s.hasGrant = true;
  s.s5StakeWei = (33500n * 10n ** 18n).toString();
  s.selfStake = 500;
  // Fabricated sim vitals pre-seeded — must NOT surface in tauri.
  s.sponsorUnits = 4;
  s.earnVal = 96.42;
  s.earnPin = 21.7;
  s.earnComp = 9.31;
  return { ...s, ...over };
}

describe("Wallet — Q-A.4b tauri display honesty", () => {
  it("HIDES the fabricated Paymaster sponsor bar in tauri (no 'of 5 units')", () => {
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={walletState("overview")} />);
    // NEGATIVE CONTROL — the fabricated "X of 5 units" sponsor line must NOT render.
    expect(html).not.toContain("of 5 units");
    expect(html).not.toContain("Paymaster");
  });

  it("staking 'rewards accrued' shows '—' in tauri, never a fabricated accrued number", () => {
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={walletState("staking")} />);
    // NEGATIVE CONTROL — the fabricated accrued sum (96.42+21.7+9.31 = 127.43) must NOT render.
    expect(html).not.toContain("127.43");
    // POSITIVE — honest em-dash on the accrued line.
    expect(html).toContain("rewards accrued · —");
  });

  it("Staked renders the REAL attributed stake (s5StakeWei), not a hardcoded 32,000", () => {
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={walletState("staking")} />);
    // POSITIVE — the real read (33,500 grant + 500 self = 34,000) is the staked total.
    expect(html).toContain("34,000");
    // POSITIVE — the "Membership grant · vaulted" row renders the REAL attributed
    // stake (33,500), not the hardcoded 32,000 constant.
    expect(html).toContain("33,500 SALT");
    // NEGATIVE CONTROL — with a grant that is NOT 32,000, the old hardcoded
    // (32,000 + 500 = 32,500) staked total must NOT appear.
    expect(html).not.toContain("32,500");
  });
});
