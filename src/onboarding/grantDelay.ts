// When S3's "Waiting for checkout…" stops being true (the 2026-11-12..11-17 grant outage).
//
// Data sources (Rule 7):
//   - chain 40204 (`membership.grantStatus`): the ONLY thing that settles S3;
//   - core-membership's public `/api/health` → `chain.signerReadiness` (`membership.signerHealth`,
//     read in Rust; core-membership#86): whether a grant can be signed at all right now;
//   - the time this app opened checkout (in memory, this session only).
// The app cannot read the order itself: core-membership's `/api/orders/current` accepts only
// tokens minted for the core-membership client, not the app's. So "delayed" never claims the
// payment went through.

/** No grant on chain this long after checkout opened = delayed. One reconciler sweep (10 min cron). */
export const GRANT_DELAYED_AFTER_MS = 10 * 60_000;

export interface GrantDelayInput {
  /** When this app opened checkout (ms), or null if it has not. */
  payingSinceMs: number | null;
  nowMs: number;
  /** core-membership's signer readiness: true ready, false a named not-ready state, null not read. */
  signerReady: boolean | null;
}

/** True when S3 should say the grant is delayed on our side instead of "Waiting for checkout…". */
export function isGrantDelayed({ payingSinceMs, nowMs, signerReady }: GrantDelayInput): boolean {
  if (payingSinceMs === null) return false;
  if (signerReady === false) return true;
  return nowMs - payingSinceMs >= GRANT_DELAYED_AFTER_MS;
}
