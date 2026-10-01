// CX bridge impl — contracts (Hermes P3 / WP3.2), TAURI.
//
// A real contract-creation deploy: the Rust `contract_deploy` command assembles the init
// code (bytecode ++ constructor args) into a `to`-less creation tx and submits a PENDING
// SignatureCeremony (Rule 3 — the human approves + broadcasts; nothing signs here). The
// bytecode is caller-supplied + compiled; the app never fabricates contract code (Rule 1).
import { invoke } from "./invoke";
import type { ContractDeployInput, ContractsDomain, DeployProposalView } from "../domains";
import type { DeployGateInputs, DeployGateLookup, DeployGateRecord } from "../../agent/deployGate";

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
};
