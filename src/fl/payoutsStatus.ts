// =====================================================================
// citrate-core: whether FL payouts are open, as core's Rust payouts gate decides it (G-SEC PAY-14)
//
// Data source (Rule 7): core's `fl_payouts_status` (src-tauri fl_payouts.rs over
// kit/src/payouts_gate.rs, #736): the active profile's `payoutsOpen` AND WorkMarket and
// FLRoundVerifier pinned with their live eth_getCode hashing to the pinned code. The shape mirrors
// its serde output. Payout and claim surfaces read this, never the profile flag alone; the signer
// enforces the same rule on every FL transaction whatever a surface shows.
// =====================================================================
import { BRIDGE_MODE } from "../bridge/mode";
import { invoke } from "../bridge/tauri/invoke";

export interface FlContractPayoutStatus {
  contract: string;
  address: string | null;
  pinnedCodeHash: string | null;
  /** null = not read (not pinned, or payouts dark). */
  deployed: boolean | null;
  codeHashMatches: boolean | null;
  reason: string | null;
}

export interface PayoutsStatus {
  network: string;
  payoutsOpenFlag: boolean;
  /** Effective: the flag is set and the payout path's live code matches its pins. */
  open: boolean;
  /** The gate's own words for why payouts are not open. */
  reason: string | null;
  contracts: FlContractPayoutStatus[];
}

export const PAYOUTS_STATUS_NEEDS_DESKTOP = "whether payouts are open is decided by the desktop app's core";

/** Ask core. Rejects outside the desktop app. */
export function readPayoutsStatus(): Promise<PayoutsStatus> {
  if (BRIDGE_MODE !== "tauri") return Promise.reject(new Error(PAYOUTS_STATUS_NEEDS_DESKTOP));
  return invoke<PayoutsStatus>("fl_payouts_status");
}
