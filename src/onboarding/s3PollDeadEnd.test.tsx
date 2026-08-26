// S3 grant-poll dead end (Rule 1 honesty + a money guard).
//
// THE BUG this pins, observed live 2026-08-15. A member paid at 12:43. The S3 poll
// was bounded at 60 × 5s = 5 minutes and then simply STOPPED, leaving `s3 === "paying"`
// — a state that rendered nothing but a spinner. Two things made that a dead end
// rather than a delay:
//
//   1. The client budget (5 min) was SHORTER than the server's own retry cadence.
//      core-membership re-drives stranded grants on a 10-minute cron, and backs off
//      6 HOURS after a denial. So in exactly the case the reconciler exists to
//      rescue, the client had already given up before the first retry could run.
//   2. The "paying" branch offered NO control. The only button the member could
//      ever see again was "Check out in your browser · $48" (the `s3 === "idle"`
//      branch, reachable by restarting the app, since s3 is not persisted as
//      "paying"). A member trying to unstick themselves would pay a SECOND time for
//      a membership they had already bought — and the grant is one-per-sub, so the
//      second payment could never be honoured either.
//
// So the exhausted state must (a) exist and say what is true, (b) offer a re-check
// that only READS the chain, and (c) NOT offer a pay affordance.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { S3 } from "./Onboarding";
import { freshState, type AppState } from "../shell/state";
import { store as realStore, type Store } from "../shell/store";

const noopStore = {} as unknown as Store;

function payingState(exhausted: boolean): AppState {
  const s = freshState("p1");
  s.stage = "s3";
  s.s3 = "paying";
  s.s3PollExhausted = exhausted;
  return s;
}

describe("S3 — the bounded grant poll must not dead-end on a spinner", () => {
  it("while still polling, shows the waiting spinner and no exhausted card", () => {
    const html = renderToStaticMarkup(<S3 store={noopStore} s={payingState(false)} />);
    expect(html).toContain("Waiting for checkout");
    expect(html).not.toContain("s3-poll-exhausted");
  });

  it("once the poll is exhausted, explains the state instead of spinning forever", () => {
    const html = renderToStaticMarkup(<S3 store={noopStore} s={payingState(true)} />);
    expect(html).toContain("s3-poll-exhausted");
    // Says the payment landed and the grant has not — the honest split.
    expect(html).toContain("Payment received");
    // Offers the read-only retry.
    expect(html).toContain("Check again");
    // And the spinner is gone (the two branches are mutually exclusive).
    expect(html).not.toContain("Waiting for checkout");
  });

  it("MONEY GUARD: the exhausted card never offers a second checkout", () => {
    const html = renderToStaticMarkup(<S3 store={noopStore} s={payingState(true)} />);
    // The member has already paid. Re-opening Stripe here would double-charge for a
    // one-per-sub membership that can only ever be granted once.
    expect(html).not.toContain("Check out in your browser");
    expect(html).not.toContain("· $48<");
    // It tells them so explicitly rather than leaving it implied.
    expect(html).toContain("you do not need");
    expect(html).toContain("rather than paying twice");
  });
});

describe("store.recheckMembership — the exit from the dead end", () => {
  it("clears the exhausted flag so the poll can resume", () => {
    realStore.setState({ s3: "paying", s3PollExhausted: true });
    realStore.recheckMembership();
    expect(realStore.state.s3PollExhausted).toBe(false);
  });

  it("is a no-op unless S3 is actually paying (never resurrects a settled stage)", () => {
    realStore.setState({ s3: "settled", s3PollExhausted: true });
    realStore.recheckMembership();
    // Untouched: the guard returned before mutating anything.
    expect(realStore.state.s3).toBe("settled");
    expect(realStore.state.s3PollExhausted).toBe(true);
  });
});
