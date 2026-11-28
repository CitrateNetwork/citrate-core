// citrate-core: bridge domain `training`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// ── C-21 train-together: group federated training (lane s5) ──
export type RoundPhase = "idle" | "open" | "aggregating" | "committed" | "settled";
export interface RoundStatus {
  groupId: string;
  round: number;
  phase: RoundPhase;
  participants: number;
}
export interface RewardInfo {
  round: number;
  /** Cumulative patronage units (PatronageLedger.units). */
  weight: string;
  /** Pending dividend in whole SALT, rounded down. */
  salt: string;
  /** The same pending dividend exactly, in wei (decimal string). */
  pendingWei: string;
}
/** PatronageLedger reads. V5 (0.5.4): no start, contribute or claim here; those were stubs that
 *  always failed. Rounds are joined and claimed through the WorkMarket tools once deployed. */
export interface TrainingDomain {
  status(groupId: string): Promise<RoundStatus>;
  reward(groupId: string): Promise<RewardInfo>;
}
