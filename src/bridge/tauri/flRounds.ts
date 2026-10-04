// Bridge impl: federated rounds (HUP-S9.4), TAURI. Every call goes to core (src-tauri
// fl_rounds.rs): the coordinator read, the plan, the HIC-1 start record and the LoRA eval gate.
import { invoke } from "./invoke";
import type {
  FlAdapterGateRecord,
  FlAdapterGateRequest,
  FlCoordinatorConfig,
  FlEvalArm,
  FlEvalSession,
  FlOverview,
  FlRoundPlan,
  FlRoundProposal,
  FlRoundProvenance,
  FlRoundsDomain,
  FlStartReceipt,
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
  importRound(bundlePath: string, adapterPath: string) {
    return invoke<FlRoundProvenance>("fl_round_import", { bundlePath, adapterPath });
  },
  fetchRound(bundleUrl: string, adapterUrl: string) {
    return invoke<FlRoundProvenance>("fl_round_fetch", { bundleUrl, adapterUrl });
  },
  fetchAdapter(url: string, expectedSha256: string) {
    return invoke<string>("fl_adapter_fetch", { url, expectedSha256 });
  },
  evalBegin(adapterPath: string, expectedSha256: string) {
    return invoke<FlEvalSession>("fl_eval_begin", { adapterPath, expectedSha256 });
  },
  evalComplete(sessionId: string, arm: FlEvalArm, messagesJson: string, toolsJson: string) {
    return invoke<string>("fl_eval_complete", { sessionId, arm, messagesJson, toolsJson });
  },
  evalFinish(sessionId: string, baseScorecardJson: string, candidateScorecardJson: string) {
    return invoke<FlAdapterGateRecord>("fl_eval_finish", { sessionId, baseScorecardJson, candidateScorecardJson });
  },
  async evalEnd(sessionId: string) {
    await invoke("fl_eval_end", { sessionId });
  },
};
