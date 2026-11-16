// CX bridge impl — training (SETL-S3), TAURI. Real 40204 reads via eth_call to PatronageLedger.
// V5 (0.5.4): the start/contribute/claim commands were removed (they always returned an error).
import { invoke } from "./invoke";
import type { RewardInfo, RoundStatus, TrainingDomain } from "../domains";

export const tauriTraining: TrainingDomain = {
  status(groupId: string): Promise<RoundStatus> {
    return invoke<RoundStatus>("training_status", { group: groupId });
  },
  reward(groupId: string): Promise<RewardInfo> {
    return invoke<RewardInfo>("training_reward", { group: groupId });
  },
};
