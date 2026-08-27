// CX bridge impl — training (C-21), TAURI. Owned by lane s5 (CX-S5) after S0.
// S0.2 stub: honest Unavailable. CX-S5 wires the round-coordinator + ceremony-gated settlement.
import type { TrainingDomain } from "../domains";
import { Unavailable } from "../types";

export const tauriTraining: TrainingDomain = {
  async start() {
    throw new Unavailable("training", "start");
  },
  async status() {
    throw new Unavailable("training", "status");
  },
  async contribute() {
    throw new Unavailable("training", "contribute");
  },
  async reward() {
    throw new Unavailable("training", "reward");
  },
  async claim() {
    throw new Unavailable("training", "claim");
  },
};
