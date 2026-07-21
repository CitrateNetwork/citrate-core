// Q-A.4b items 9 & 10 — onboarding display honesty:
//   9. The S3 "Payment settled" card showed a STATIC fabricated order id
//      (ord_2026_84117) as if real → drop the id line (no real order id is
//      exposed by the entitlement/userinfo claim).
//   10. S4 derived the wallet address from the persona (makeAddr) when a signed-in
//      user has no real wallet_address claim → show "—"/pending for a signed-in
//      user without a real wallet claim, never a fabricated persona address.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { S3, S4 } from "./Onboarding";
import { freshState, makeAddr, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

// S3/S4 render from `s`; `store` is only touched in button onClicks (never fired
// by a static render).
const noopStore = {} as unknown as Store;

function s3Settled(): AppState {
  const s = freshState("p1");
  s.stage = "s3";
  s.s3 = "settled";
  return s;
}

describe("Onboarding S3 settled card — item 9 (fake order id)", () => {
  it("does NOT render the fabricated static order id ord_2026_84117", () => {
    const html = renderToStaticMarkup(<S3 store={noopStore} s={s3Settled()} />);
    // NEGATIVE CONTROL — the fabricated order id is gone.
    expect(html).not.toContain("ord_2026_84117");
    // POSITIVE — the settled confirmation itself still renders.
    expect(html).toContain("Payment settled");
  });
});

describe("Onboarding S4 wallet — item 10 (persona-derived address)", () => {
  it("shows '—'/pending for a signed-in user with NO real wallet claim (never a persona address)", () => {
    const s = freshState("p1");
    s.stage = "s4";
    s.signedIn = true;
    s.walletFromClaim = false; // no real wallet_address claim folded
    const html = renderToStaticMarkup(<S4 store={noopStore} s={s} />);
    // NEGATIVE CONTROL — the fabricated persona address must NOT render.
    expect(html).not.toContain(makeAddr("Dana Okafor"));
    expect(html).not.toContain(s.walletAddr);
    // POSITIVE — an honest pending state instead of a fabricated address.
    expect(html.toLowerCase()).toContain("pending");
  });

  it("renders the REAL wallet address for a signed-in user WITH a real wallet claim (positive control)", () => {
    const s = freshState("p1");
    s.stage = "s4";
    s.signedIn = true;
    s.walletFromClaim = true;
    s.walletAddr = "0x" + "ab".repeat(20);
    const html = renderToStaticMarkup(<S4 store={noopStore} s={s} />);
    expect(html).toContain("0x" + "ab".repeat(20));
  });

  it("renders the wallet address in the sim/persona web-dev path (not signed in)", () => {
    // In web-dev (not signed in) the deterministic persona address is the honest
    // preview — the persona is a labelled web-dev affordance, so it may render.
    const s = freshState("p1");
    s.stage = "s4";
    s.signedIn = false;
    const html = renderToStaticMarkup(<S4 store={noopStore} s={s} />);
    expect(html).toContain(s.walletAddr);
  });
});
