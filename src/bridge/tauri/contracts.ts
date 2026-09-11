// CX bridge impl — contracts (Hermes P3 / WP3.2), TAURI.
//
// A real contract-creation deploy: the Rust `contract_deploy` command assembles the init
// code (bytecode ++ constructor args) into a `to`-less creation tx and submits a PENDING
// SignatureCeremony (Rule 3 — the human approves + broadcasts; nothing signs here). The
// bytecode is caller-supplied + compiled; the app never fabricates contract code (Rule 1).
import { invoke } from "./invoke";
import type { ContractDeployInput, ContractsDomain } from "../domains";
import type { CeremonyView } from "../types";

export const tauriContracts: ContractsDomain = {
  deploy(input: ContractDeployInput) {
    // camelCase arg keys — Tauri v2 maps them to the Rust command's snake_case params.
    return invoke<CeremonyView>("contract_deploy", {
      bytecodeHex: input.bytecodeHex,
      constructorArgsHex: input.constructorArgsHex ?? null,
      valueWei: input.valueWei ?? null,
      gas: input.gas ?? null,
    });
  },
};
