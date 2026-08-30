// CX bridge impl — training (SETL-S3), TAURI. Real 40204 reads via eth_call to PatronageLedger;
// contribute/claim are honestly gated (SETTLER-only recording + @rule8/gateSec on member SALT).
import { invoke } from "@tauri-apps/api/core";
import type { RewardInfo, RoundStatus, TrainingDomain } from "../domains";

export const tauriTraining: TrainingDomain = {
  async start(groupId: string): Promise<void> {
    await invoke("training_start", { group: groupId });
  },
  status(groupId: string): Promise<RoundStatus> {
    return invoke<RoundStatus>("training_status", { group: groupId });
  },
  async contribute(groupId: string): Promise<void> {
    await invoke("training_contribute", { group: groupId });
  },
  reward(groupId: string): Promise<RewardInfo> {
    return invoke<RewardInfo>("training_reward", { group: groupId });
  },
  async claim(groupId: string): Promise<void> {
    await invoke("training_claim", { group: groupId });
  },
};
