// Test fixture (HUP-S9.4): a plan in the exact shape core's fl_round_plan returns. Imported only
// by tests; production reads plans from core.
import type { FlRoundPlan } from "../../bridge/domains";
import { DEFAULT_PROPOSAL } from "../flRounds";

export const PLAN_HASH = "a".repeat(64);

export function livePlan(over: Partial<FlRoundPlan> = {}): FlRoundPlan {
  return {
    planHash: PLAN_HASH,
    createdAtMs: 1,
    coordinator: {
      state: "live",
      url: "https://coordinator.example.org",
      status: { pending: 3, leased: 1, done: 0, quarantined: 0, workers: 4, settlement: "shadow", phase: "running" },
    },
    proposal: { ...DEFAULT_PROPOSAL },
    baseModel: "gemma-4-E4B-it-Q4_0.gguf",
    device: { tier: "T1", accelerator: true },
    explain: {
      data: "Trains only on your verified Hermes conversations, at most 500 of them.",
      compute: "The coordinator reports 4 machines registered, 3 jobs waiting.",
      reward: "The coordinator runs settlement in shadow mode: contributions are measured and nothing is paid for this round.",
      privacy: "Your conversations never leave this device.",
      status: "The coordinator at https://coordinator.example.org has work open.",
    },
    canStart: true,
    blockers: [],
    ...over,
  };
}

