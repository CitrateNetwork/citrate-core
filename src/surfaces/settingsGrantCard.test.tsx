// BC-1 (F1 — Rule 1 display honesty) — the Settings "SALT grant record" card
// (billing section) must render ONLY values that trace to a real read, mirroring
// the S5 settled-card fix.
//
// Before the fix this card showed a hardcoded "32,000 SALT" and a fabricated
// "Transaction: {short(s5hash)}" row (s5hash is a persona makeHash(), NOT a real
// tx — this app never broadcasts the grant tx). We assert:
//   - the staked amount is FORMATTED FROM the real grant read (s5StakeWei,
//     wei→SALT) — a distinct, non-32,000 value proves it is not hardcoded;
//   - no "Transaction" row and no fabricated tx-hash is present;
//   - the honest on-chain anchor is the member wallet;
//   - "—" (never a fabricated number) renders when the real read is absent.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Settings } from "./Settings";
import { freshState, fmtSaltFromWei, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

// The grant card renders purely from `s` (AppState). The only render-time store
// call is `store.identity()` (account claims, elsewhere on the page); Settings'
// useEffect hooks never fire during a static server render. A minimal identity
// stub is enough to render the full page and reach the grant card.
const stubStore = {
  identity: () => ({
    name: "Member",
    initials: "M",
    email: "member@example.com",
    sub: "sub-1",
    wallet: "0xC0ffee0000000000000000000000000000ABCDEF",
    tier: "core",
    role: "member",
    org: null,
    real: true,
  }),
} as unknown as Store;

function grantState(stakeWei: string | null): AppState {
  const s = freshState("p1");
  s.sSec = "billing"; // the "SALT grant record" card lives in the billing section
  s.hasSbt = true; // card is gated on a minted membership SBT
  s.s5StakeWei = stakeWei;
  s.walletAddr = "0xC0ffee0000000000000000000000000000ABCDEF";
  return s;
}

describe("Settings grant card — F1 display honesty", () => {
  it("renders the REAL staked value from the grant read (s5StakeWei), not a hardcoded 32000", () => {
    // A DELIBERATELY non-32,000 real read (33,500 SALT). If the card were
    // hardcoded to 32,000 this assertion would fail — the value must trace to
    // the real attributedStake read threaded into s5StakeWei.
    const realWei = (33500n * 10n ** 18n).toString();
    const html = renderToStaticMarkup(<Settings store={stubStore} s={grantState(realWei)} />);

    expect(fmtSaltFromWei(realWei)).toBe("33,500");
    expect(html).toContain("33,500");
    // And it is NOT the previously-hardcoded literal.
    expect(html).not.toContain("32,000");
  });

  it("shows no fabricated Transaction row or tx-hash on the grant card", () => {
    const realWei = (33500n * 10n ** 18n).toString();
    const html = renderToStaticMarkup(<Settings store={stubStore} s={grantState(realWei)} />);

    // The fabricated "Transaction" row (which rendered short(s5hash)) is gone —
    // this app does not broadcast the grant tx, so there is no honest tx hash.
    expect(html).not.toContain("Transaction");
    // The honest on-chain anchor is the member wallet.
    expect(html).toContain("Member wallet");
    expect(html).toContain("0xC0ff");
  });

  it("renders '—' (never a fabricated number) when the real stake read is absent", () => {
    const html = renderToStaticMarkup(<Settings store={stubStore} s={grantState(null)} />);
    expect(html).not.toContain("32,000");
    // fmtSaltFromWei(null) === "—"
    expect(html).toContain("—");
  });
});
