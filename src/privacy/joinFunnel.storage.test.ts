// 0.5.2 Join — storage that READS but cannot WRITE (for example a localStorage quota error).
// Written red-first: before this, a failed write updated only the in-memory copy, while every
// later read returned the stale persisted record, so dedupe and the consent version regressed.
//
// Rule pinned here: once a write fails, the in-memory copy wins over what is persisted for the
// rest of the session (sticky), for the step sets AND for the remembered consent version.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";

const FUNNEL_KEY = "citrate-core-join-funnel-v1";
const STATE_KEY = "citrate-core-proto-v2";

type Funnel = typeof import("./joinFunnel");
let f: Funnel;

beforeEach(async () => {
  localStorage.clear();
  vi.resetModules(); // fresh module state (memory copy + sticky flag) per test
  f = await import("./joinFunnel");
});
afterEach(() => {
  vi.restoreAllMocks();
});

/** Reads keep working; every write throws, the way a full quota does. */
function failWrites() {
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new DOMException("quota exceeded", "QuotaExceededError");
  });
  vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => {
    throw new DOMException("quota exceeded", "QuotaExceededError");
  });
}

describe("join funnel when storage writes fail", () => {
  it("step sets: dedupe holds and nothing already recorded is lost", () => {
    f.setJoinFunnelConsent(() => true);
    f.reachedStep("open_app"); // persisted normally
    failWrites();
    f.reachedStep("sign_in"); // write fails: memory only
    expect(f.pendingJoinSteps()).toEqual(["open_app", "sign_in"]);
    f.reachedStep("sign_in");
    expect(f.pendingJoinSteps()).toEqual(["open_app", "sign_in"]);
    f.markJoinStepsSent(["open_app", "sign_in"]);
    expect(f.pendingJoinSteps()).toEqual([]);
    f.reachedStep("open_app"); // stale persisted record still says pending; memory says sent
    f.reachedStep("sign_in");
    expect(f.pendingJoinSteps()).toEqual([]);
  });

  it("clearing still forgets the steps when the removal cannot be written", () => {
    f.setJoinFunnelConsent(() => true);
    f.reachedStep("wallet");
    failWrites();
    f.clearJoinSteps();
    expect(f.pendingJoinSteps()).toEqual([]);
  });

  it("consent version: an acknowledgement held in memory wins over the stale persisted marker", () => {
    // Persisted record: a pre-0.5.2 opt-in (version 1). No live source bound, so the fallback runs.
    localStorage.setItem(STATE_KEY, JSON.stringify({ telemetry: true, telemetryConsentVersion: 1 }));
    f.setJoinFunnelConsent(null);
    failWrites();
    f.reachedStep("kyc");
    expect(f.pendingJoinSteps()).toEqual([]); // before the note: nothing
    f.rememberTelemetryConsent(f.telemetryConsentPatch(true)); // the member keeps it on
    f.reachedStep("kyc");
    expect(f.pendingJoinSteps()).toEqual(["kyc"]);
    f.rememberTelemetryConsent(f.telemetryConsentPatch(false)); // and later turns it off
    f.reachedStep("bond");
    expect(f.pendingJoinSteps()).toEqual([]);
  });

  it("with working storage, the persisted record is still the source", () => {
    f.setJoinFunnelConsent(() => true);
    f.reachedStep("arrive");
    localStorage.setItem(FUNNEL_KEY, JSON.stringify({ v: 1, pending: [], sent: ["arrive"] }));
    expect(f.pendingJoinSteps()).toEqual([]);
  });
});
