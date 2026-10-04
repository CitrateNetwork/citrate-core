// CX bridge impl — contracts (Hermes P3 / WP3.2), TAURI.
//
// A real contract-creation deploy: the Rust `contract_deploy` command assembles the init
// code (bytecode ++ constructor args) into a `to`-less creation tx and submits a PENDING
// SignatureCeremony (Rule 3 — the human approves + broadcasts; nothing signs here). The
// bytecode is caller-supplied + compiled; the app never fabricates contract code (Rule 1).
import { invoke } from "./invoke";
import type { CeremonyView } from "../types";
import type {
  ContractDeployInput,
  ContractSourceView,
  ContractsDomain,
  DeployProposalView,
  DeployReceiptView,
  PostDeployStatus,
  SitePinView,
  VercelExportView,
  VerifyOutcomeView,
} from "../domains";
import type {
  DeployGateInputs,
  DeployGateLookup,
  DeployGateRecord,
  ForkDryRunInput,
  ForkDryRunRequest,
} from "../../agent/deployGate";
import type { VerifiedSourceView } from "../../agent/verifiedSource";

export const tauriContracts: ContractsDomain = {
  deploy(input: ContractDeployInput) {
    // camelCase arg keys — Tauri v2 maps them to the Rust command's snake_case params.
    return invoke<DeployProposalView>("contract_deploy", {
      bytecodeHex: input.bytecodeHex,
      constructorArgsHex: input.constructorArgsHex ?? null,
      valueWei: input.valueWei ?? null,
      gas: input.gas ?? null,
    });
  },
  // HUP-S6.4 — the D-4 deploy gate (core parses the verifier outputs; the verdict is core's).
  gateLookup(bytecodeHex: string, constructorArgsHex?: string) {
    return invoke<DeployGateLookup>("deploy_gate_lookup", { bytecodeHex, constructorArgsHex: constructorArgsHex ?? null });
  },
  gateSubmit(inputs: DeployGateInputs) {
    return invoke<DeployGateRecord>("deploy_gate_submit", { inputs });
  },
  // HUP-S6.10 — the fork step on the Citrate-aware fork (core runs citrate-fork, read-only).
  gateForkDryRun(request: ForkDryRunRequest) {
    return invoke<ForkDryRunInput>("deploy_gate_fork_dry_run", { request });
  },
  // HUP-S4.3 — read-only CitrateScan verified-source lookup (core makes the HTTP call).
  verifiedSource(address: string) {
    return invoke<VerifiedSourceView>("contract_verified_source", { address });
  },
  // HUP-S6.7 — the Contract reader (reads; a write only opens a ceremony).
  source(address: string) {
    return invoke<ContractSourceView>("contract_source", { address });
  },
  codeSize(target: string, address: string) {
    return invoke<number>("contract_code_size", { target, address });
  },
  viewCall(target: string, address: string, calldata: string) {
    return invoke<string>("contract_view_call", { target, address, calldata });
  },
  proposeWrite(address: string, calldata: string, valueWei: string) {
    return invoke<CeremonyView>("contract_write_propose", { address, calldata, valueWei });
  },
  // HUP-S6.6 — after the deploy.
  postdeployStatus(projectDir: string) {
    return invoke<PostDeployStatus>("postdeploy_status", { projectDir });
  },
  postdeployReceipt(txHash: string) {
    return invoke<DeployReceiptView | null>("postdeploy_receipt", { txHash });
  },
  postdeployVerify(projectDir: string, address: string, constructorArgsHex?: string) {
    return invoke<VerifyOutcomeView>("postdeploy_verify", { projectDir, address, constructorArgsHex: constructorArgsHex ?? null });
  },
  postdeploySwitchSite(projectDir: string, address: string) {
    return invoke<{ envPath: string; address: string }>("postdeploy_switch_site", { projectDir, address });
  },
  postdeployPinSite(projectDir: string) {
    return invoke<SitePinView>("postdeploy_pin_site", { projectDir });
  },
  postdeployVercelExport(projectDir: string) {
    return invoke<VercelExportView>("postdeploy_vercel_export", { projectDir });
  },
};
