// Bridge impl: federated rounds (HUP-S9.4), TAURI. Every call goes to core (src-tauri
// fl_rounds.rs): the coordinator read, the plan, the HIC-1 start record and the LoRA eval gate.
import { invoke } from "./invoke";
import type {
  FlAdapterGateRecord,
  FlAdapterGateRequest,
  FlCoordinatorConfig,
  FlOverview,
  FlRoundPlan,
  FlRoundProposal,
  FlRoundsDomain,
  FlStartReceipt,
  FlTrainingSet,
  FlTrajectoryStatus,
} from "../domains";

export const tauriFlRounds: FlRoundsDomain = {
  overview() {
    return invoke<FlOverview>("fl_overview");
  },
  setCoordinator(url: string | null) {
    return invoke<FlCoordinatorConfig>("fl_coordinator_set", { url });
  },
  plan(proposal?: FlRoundProposal) {
    return invoke<FlRoundPlan>("fl_round_plan", { proposal: proposal ?? null });
  },
  lookupPlan(planHash: string) {
    return invoke<FlRoundPlan>("fl_round_plan_lookup", { planHash });
  },
  start(planHash: string) {
    return invoke<FlStartReceipt>("fl_round_start", { planHash });
  },
  gateAdapter(request: FlAdapterGateRequest) {
    return invoke<FlAdapterGateRecord>("fl_adapter_gate", { request });
  },
  loadAdapter(sha256: string) {
    return invoke<string>("fl_adapter_load", { sha256 });
  },
  async unloadAdapter() {
    await invoke("fl_adapter_unload");
  },
  revokeConsent(roundId: string) {
    return invoke<string[]>("fl_round_consent_revoke", { roundId });
  },
  trajectoryStatus() {
    return invoke<FlTrajectoryStatus>("trajectories_settings_get");
  },
  setTrajectoryConsent(enabled: boolean, deleteRecorded: boolean) {
    return invoke<FlTrajectoryStatus>("trajectories_settings_set", { enabled, deleteRecorded });
  },
  buildTrainingSet(maxTrajectories: number) {
    return invoke<FlTrainingSet>("trajectories_dataset_build", { maxTrajectories });
  },
};
