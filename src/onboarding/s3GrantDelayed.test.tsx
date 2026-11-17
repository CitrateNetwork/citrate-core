// S3 "Waiting for checkout…" must not be an endless wait (the 2026-11-12..11-17 grant outage).
//
// THE INCIDENT. At the r1007 ceremony the treasury-signer container on citrate-identity was
// stopped, and its env still held the retired r1006 grantor key. No membership grant could be
// signed for five days. A member who paid sat on the S3 spinner for the poll's whole ~28 minute
// budget with nothing to say why, because the app only read the chain, and the chain had
// nothing to report.
//
// THE FIX this pins. The chain stays the only thing that settles S3. The app ALSO reads
// core-membership's public `/api/health` (its `chain.signerReadiness`, core-membership#86), and
// the S3 card stops claiming "Waiting for checkout…" when either:
//   - core-membership reports the grant signer not ready, or
//   - 10 minutes have passed since checkout opened with no grant on chain (one reconciler sweep).
// Then it says the grant is delayed on our side, that it will finish automatically, that there
// is no need to pay again, and offers "Check again" (a read) and a support contact. It NEVER
// offers to pay again.
//
// Honesty note: the app cannot read the order itself. core-membership's `/api/orders/current`
// only accepts tokens minted for the core-membership client, not the app's token. So the card
// does not claim the payment went through; it says what to expect if it did.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { S3 } from "./Onboarding";
import { GRANT_DELAYED_AFTER_MS, isGrantDelayed } from "./grantDelay";
import { freshState, type AppState } from "../shell/state";
import { store as realStore, type Store } from "../shell/store";
import { createSimBridge } from "../bridge/sim";

const noopStore = {} as unknown as Store;

function delayedState(signerReady: boolean | null): AppState {
  const s = freshState("p1");
  s.stage = "s3";
  s.s3 = "paying";
  s.s3PollExhausted = false;
  s.s3GrantDelayed = true;
  s.s3SignerReady = signerReady;
  return s;
}

describe("isGrantDelayed — when the S3 wait stops being 'waiting for checkout'", () => {
  const t0 = 1_700_000_000_000;

  it("is not delayed in the normal settle window with a ready (or unread) signer", () => {
    expect(isGrantDelayed({ payingSinceMs: t0, nowMs: t0 + 60_000, signerReady: true })).toBe(false);
    expect(isGrantDelayed({ payingSinceMs: t0, nowMs: t0 + 60_000, signerReady: null })).toBe(false);
  });

  it("is delayed at once when core-membership reports the signer not ready", () => {
    expect(isGrantDelayed({ payingSinceMs: t0, nowMs: t0 + 1_000, signerReady: false })).toBe(true);
  });

  it("is delayed after N minutes with no grant on chain, whatever the signer read", () => {
    expect(GRANT_DELAYED_AFTER_MS).toBe(10 * 60_000);
    expect(isGrantDelayed({ payingSinceMs: t0, nowMs: t0 + GRANT_DELAYED_AFTER_MS - 1, signerReady: null })).toBe(false);
    expect(isGrantDelayed({ payingSinceMs: t0, nowMs: t0 + GRANT_DELAYED_AFTER_MS, signerReady: null })).toBe(true);
    expect(isGrantDelayed({ payingSinceMs: t0, nowMs: t0 + GRANT_DELAYED_AFTER_MS, signerReady: true })).toBe(true);
  });

  it("is never delayed before checkout opened", () => {
    expect(isGrantDelayed({ payingSinceMs: null, nowMs: t0, signerReady: null })).toBe(false);
  });
});

describe("S3 — the delayed-grant card replaces the endless spinner", () => {
  it("while not delayed, still shows the waiting spinner and no delayed card", () => {
    const s = delayedState(null);
    s.s3GrantDelayed = false;
    const html = renderToStaticMarkup(<S3 store={noopStore} s={s} />);
    expect(html).toContain("Waiting for checkout");
    expect(html).not.toContain("s3-grant-delayed");
  });

  it("when delayed by time, says the membership has not landed and, IF paid, will finish automatically", () => {
    const html = renderToStaticMarkup(<S3 store={noopStore} s={delayedState(null)} />);
    expect(html).toContain("s3-grant-delayed");
    expect(html).not.toContain("Waiting for checkout");
    expect(html).toContain("Your membership has not landed yet");
    expect(html).toContain("in 10 minutes");
    expect(html).toContain("If your payment went through, the delay is on our side");
    expect(html).toContain("finish it automatically");
    expect(html).toContain("No need to pay again");
    expect(html).toContain("Check again");
    expect(html).toContain("support@citrate.ai");
  });

  it("names the signer outage when core-membership reports the signer not ready", () => {
    const html = renderToStaticMarkup(<S3 store={noopStore} s={delayedState(false)} />);
    expect(html).toContain("Membership grants are paused on our side right now");
    expect(html).toContain("once grants resume");
  });

  // The signer read lands on the first poll tick, 5 s after checkout opened, when the member may
  // still be in checkout or may never pay. So the paused card assumes no payment.
  it("the signer-paused card assumes no payment (it can show before checkout ends)", () => {
    const html = renderToStaticMarkup(<S3 store={noopStore} s={delayedState(false)} />);
    expect(html).toContain("If you are still in checkout, you can finish it");
    expect(html).toContain("If your payment went through");
    expect(html).not.toContain("Your membership grant is delayed");
    expect(html).not.toContain("in 10 minutes");
  });

  it("neither delayed card claims a payment, names a grant amount, or uses an em-dash", () => {
    for (const ready of [false, null]) {
      const html = renderToStaticMarkup(<S3 store={noopStore} s={delayedState(ready)} />);
      const start = html.indexOf('data-testid="s3-grant-delayed"');
      const card = html.slice(start, html.indexOf("with the email you paid with", start));
      expect(start).toBeGreaterThan(-1);
      for (const claim of ["Payment received", "payment settled", "recorded and owed", "32,000", "SALT", "\u2014"]) {
        expect(card).not.toContain(claim);
      }
    }
  });

  it("MONEY GUARD: the delayed card never offers a second checkout", () => {
    for (const ready of [false, null]) {
      const html = renderToStaticMarkup(<S3 store={noopStore} s={delayedState(ready)} />);
      expect(html).not.toContain("Check out in your browser");
      expect(html).not.toContain("Continue free");
      expect(html).not.toContain("Pay again");
    }
  });

  it("the exhausted card still wins once the poll budget is spent", () => {
    const s = delayedState(false);
    s.s3PollExhausted = true;
    const html = renderToStaticMarkup(<S3 store={noopStore} s={s} />);
    expect(html).toContain("s3-poll-exhausted");
    expect(html).not.toContain("s3-grant-delayed");
  });
});

describe("store — the delayed flag is session state and Check again stays a read", () => {
  it("fresh state is not delayed and has no signer read", () => {
    const s = freshState("p1");
    expect(s.s3GrantDelayed).toBe(false);
    expect(s.s3SignerReady).toBe(null);
  });

  it("recheckMembership keeps S3 paying (it never re-opens checkout)", () => {
    realStore.setState({ s3: "paying", s3PollExhausted: false, s3GrantDelayed: true });
    realStore.recheckMembership();
    expect(realStore.state.s3).toBe("paying");
  });
});

describe("bridge — the signer health read has no stand-in in the web preview", () => {
  it("the sim bridge rejects instead of inventing a signer state (Rule 1)", async () => {
    await expect(createSimBridge({} as never).membership.signerHealth()).rejects.toBeTruthy();
  });
});
