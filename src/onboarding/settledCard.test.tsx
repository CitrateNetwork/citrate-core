// BC-1.3 (F1 — Rule 1 display honesty) — the S5 "settled" card must render ONLY
// values that trace to a real read, and must NOT show any fabricated identifier.
//
// This is the regression guard for finding F1: before the fix the card showed a
// hardcoded "32,000", a hardcoded "CitrateMemberSBT #4187", and a fabricated
// "Transaction: {s5hash}" row (s5hash is a persona makeHash(), NOT a real tx —
// this app never broadcasts the grant tx). We assert:
//   - the settled staked value is FORMATTED FROM the real grant read
//     (s5StakeWei, wei→SALT) — a distinct, non-32,000 value proves it is not
//     hardcoded;
//   - no "Transaction" row and no fabricated tx-hash is present;
//   - no hardcoded SBT token id ("#4187") is present.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { S5 } from "./Onboarding";
import { freshState, fmtSaltFromWei, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

// The S5 settled card renders purely from `s` (AppState); `store` is only touched
// in the "Continue" button's onClick, which never fires during a static render.
const noopStore = {} as unknown as Store;

function settledState(stakeWei: string): AppState {
  const s = freshState("p1");
  s.stage = "s5";
  s.s5 = "settled";
  s.hasGrant = true;
  s.hasSbt = true;
  s.s5StakeWei = stakeWei;
  return s;
}

describe("S5 settled card — F1 display honesty", () => {
  it("renders the REAL staked value from grantStatus (s5StakeWei), not a hardcoded 32000", () => {
    // A DELIBERATELY non-32,000 real read (33,500 SALT). If the card were
    // hardcoded to 32,000 this assertion would fail — the value must trace to the
    // real attributedStake read threaded into s5StakeWei.
    const realWei = (33500n * 10n ** 18n).toString();
    const html = renderToStaticMarkup(<S5 store={noopStore} s={settledState(realWei)} />);

    // The big number + the "Staked position" row both render the formatted real read.
    expect(fmtSaltFromWei(realWei)).toBe("33,500");
    expect(html).toContain("33,500");
    // And it is NOT the previously-hardcoded literal.
    expect(html).not.toContain("32,000");
  });

  it("shows no fabricated Transaction row or tx-hash on the settled card", () => {
    const realWei = (32000n * 10n ** 18n).toString();
    const html = renderToStaticMarkup(<S5 store={noopStore} s={settledState(realWei)} />);

    // The fabricated "Transaction" row (which rendered short(s5hash)) is gone —
    // this app does not broadcast the grant tx, so there is no honest tx hash.
    expect(html).not.toContain("Transaction");
    // The honest on-chain anchor is the member wallet, not a fake hash.
    expect(html).toContain("Member wallet");
  });

  it("drops the fabricated SBT token id — shows only 'minted'", () => {
    const realWei = (32000n * 10n ** 18n).toString();
    const html = renderToStaticMarkup(<S5 store={noopStore} s={settledState(realWei)} />);

    // No hardcoded token id (mint is confirmed by balanceOf==1; the id was never read).
    expect(html).not.toContain("#4187");
    expect(html).toContain("CitrateMemberSBT · minted");
  });

  it("renders '—' (never a fabricated number) when the real stake read is absent", () => {
    // A settled card with no threaded read must fail honest, not invent a number.
    const s = settledState("32000000000000000000000");
    s.s5StakeWei = null;
    const html = renderToStaticMarkup(<S5 store={noopStore} s={s} />);
    expect(html).not.toContain("32,000");
    expect(html).toContain("—");
  });
});
