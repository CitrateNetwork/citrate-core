// CX bridge impl — training (C-21), SIM. Owned by lane s5 after S0. Honest-empty (Rule 1).
import type { TrainingDomain, RoundStatus, RewardInfo } from "../domains";
import type { SimHost } from "./index";

export function simTraining(_host: SimHost): TrainingDomain {
  return {
    async start() {
      /* sim: no round */
    },
    async status(groupId): Promise<RoundStatus> {
      return { groupId, round: 0, phase: "idle", participants: 0 };
    },
    async contribute() {
      /* sim: no-op */
    },
    async reward(): Promise<RewardInfo> {
      return { round: 0, weight: "0", salt: "0" };
    },
    async claim() {
      /* sim: no payout */
    },
  };
}
