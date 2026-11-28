// citrate-core: bridge domain `contracts`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

import type { TemplateCatalog, TemplateRenderInput, TemplateRenderView, ToolchainGateRequest, ToolchainGateResult, ToolchainStatus } from "../../agent/contractForge";
import type { AbiResolutionView, BookLookupView, CodeInfoView, IdentityView, SignatureMatchesView, TxFetchView } from "../../agent/contractIntel";
import type { DeployGateInputs, DeployGateLookup, DeployGateRecord, ForkDryRunInput, ForkDryRunRequest } from "../../agent/deployGate";
import type { VerifiedSourceView } from "../../agent/verifiedSource";
import type { CeremonyView } from "../types";

/** The fields a contract-creation deploy needs (Hermes P3 / WP3.2). The bytecode is
 *  caller-supplied + compiled — the app never fabricates contract code (Rule 1). */
export interface ContractDeployInput {
  /** Compiled deploy bytecode (`0x`-hex or bare hex). Required. */
  bytecodeHex: string;
  /** ABI-encoded constructor args (`0x`-hex), or omit for a no-arg constructor. */
  constructorArgsHex?: string;
  /** Wei to send with the creation (decimal string), or omit for 0. */
  valueWei?: string;
  /** Gas limit (from a fork-sim estimate), or omit for the default. */
  gas?: number;
}

/** HUP-S6.4 — what `contract_deploy` returns: the pending ceremony view plus the READY D-4
 *  gate record core checked for exactly this init code. */
export type DeployProposalView = CeremonyView & { gate: DeployGateRecord };

export interface ContractsDomain {
  /** Hermes P3 / WP3.2 — propose deploying a compiled contract. Assembles the init code
   *  and submits a PENDING SignatureCeremony carrying the `to`-less creation tx (Rule 3 —
   *  the human approves + broadcasts via signing.broadcast(view.id); nothing signs here).
   *  Returns the decoded CeremonyView (a "contract creation") for the review modal. Empty
   *  bytecode rejects. HUP-S6.4: rejects with the honest refusal (naming the failing gate
   *  items) unless the D-4 deploy gate is READY for exactly this init code. */
  deploy(input: ContractDeployInput): Promise<DeployProposalView>;
  /** HUP-S6.4 — the init-code hash a deploy would carry and its gate record (null if none). */
  gateLookup(bytecodeHex: string, constructorArgsHex?: string): Promise<DeployGateLookup>;
  /** HUP-S6.4 — hand verifier outputs to core; core parses them and stores the verdict. */
  gateSubmit(inputs: DeployGateInputs): Promise<DeployGateRecord>;
  /** HUP-S6.10 — run the gate's fork step on the Citrate-aware fork; returns `forkDryRun` for
   *  `gateSubmit`. Read-only (reads chain state, signs and sends nothing). */
  gateForkDryRun(request: ForkDryRunRequest): Promise<ForkDryRunInput>;
  /** HUP-S4.3 — a contract's verified source/ABI/compiler from CitrateScan (read-only). Rejects
   *  only when the lookup itself fails; "not verified" is a normal answer. */
  verifiedSource(address: string): Promise<VerifiedSourceView>;
  /** HUP-S6.7 — CitrateScan's verified source and ABI for an address (`contract_source`). */
  source(address: string): Promise<ContractSourceView>;
  /** HUP-S6.7 — deployed code size at `address` on `target` ("citrate" or a loopback fork URL). */
  codeSize(target: string, address: string): Promise<number>;
  /** HUP-S6.7 — a read-only eth_call; resolves with the raw `0x` return data. */
  viewCall(target: string, address: string, calldata: string): Promise<string>;
  /** Board 01 — the same eth_call pinned to the RPC's head block (`contract_view_call_at`): the
   *  data, the block it ran at and the RPC host, so Hermes's answer names its block. */
  viewCallAt(target: string, address: string, calldata: string): Promise<{ data: string; block: number; rpc: string }>;
  /** HUP-S6.7 — a write call on 40204 as a PENDING SignatureCeremony (nothing signs here). */
  proposeWrite(address: string, calldata: string, valueWei: string): Promise<CeremonyView>;
  /** CORE-A1 — find a contract by name or address: the embedded 40204 book + on-chain registries. */
  addressBookLookup(query: string): Promise<BookLookupView>;
  /** CORE-A1 — code size, code hash, proxy target and verified status of an address. */
  getCode(address: string): Promise<CodeInfoView>;
  /** CORE-A1 — what a contract is: book name, ABI source, ERC-165 / ERC-20 probes, code selectors. */
  identify(address: string): Promise<IdentityView>;
  /** CORE-A2 — the ABI to decode calls to an address (verified, then bundled book ABI, then partial). */
  abiResolve(address: string): Promise<AbiResolutionView>;
  /** CORE-A2 — selectors / topics in the bundled signature subset (no network). */
  signatureLookup(selectors: string[], topics: string[]): Promise<SignatureMatchesView>;
  /** CORE-A2 — a transaction and its receipt from the 40204 RPC. */
  txFetch(hash: string): Promise<TxFetchView>;
  /** HUP-S6.6 — where a hello-mint project stands after its deploy (local reads). */
  postdeployStatus(projectDir: string): Promise<PostDeployStatus>;
  /** HUP-S6.6 — the deploy tx's receipt on 40204; null while pending. */
  postdeployReceipt(txHash: string): Promise<DeployReceiptView | null>;
  /** HUP-S6.6 — submit the project contract's source to CitrateScan's verifier. */
  postdeployVerify(projectDir: string, address: string, constructorArgsHex?: string): Promise<VerifyOutcomeView>;
  /** HUP-S6.6 — point the page at chain 40204 and the deployed contract. */
  postdeploySwitchSite(projectDir: string, address: string): Promise<{ envPath: string; address: string; decodedCalls?: boolean; decodeNote?: string }>;
  /** HUP-S6.6 — pin the built page to the app's IPFS daemon. */
  postdeployPinSite(projectDir: string): Promise<SitePinView>;
  /** HUP-S6.6 — write the Vercel-ready export folder (no account actions). */
  postdeployVercelExport(projectDir: string): Promise<VercelExportView>;
  /** CORE-A3 — keep a durable, encrypted record of a confirmed deploy of this project (core reads
   *  the receipt and transaction from 40204, the forge artifact and the gate record). */
  deployRecordSave(projectDir: string, txHash: string): Promise<DeployRecordSavedView>;
  /** CORE-A3 — this project's saved deploy records, oldest first (from its sealed file). */
  deployRecords(projectDir: string): Promise<DeployRecordSummaryView[]>;
  /** CORE-A3 — one saved record in full (ABI, constructor arguments, compiler settings,
   *  standard-JSON input, gate record) by contract address; null when none is saved. */
  deployRecordGet(projectDir: string, address: string): Promise<DeployRecordView | null>;
  /** HUP-S6.2 / US-6.4 — the bundled contract templates with their parameter forms, the tier in
   *  effect and its Medusa budget. Read-only. */
  templateList(): Promise<TemplateCatalog>;
  /** HUP-S6.2 / US-6.4 — render a template into a folder granted for writing (core validates
   *  every parameter and refuses before writing anything). */
  templateRender(input: TemplateRenderInput): Promise<TemplateRenderView>;
  /** HUP-S6.3 — the toolchain switch (off by default; applies when Hermes next starts). */
  toolchainSettings(): Promise<ToolchainStatus>;
  toolchainSetEnabled(enabled: boolean): Promise<ToolchainStatus>;
  /** HUP-S6.3 → S6.4 — run the deploy gate on a Hermes session's toolchain reports for one
   *  forge artifact; core re-parses the raw reports and stores the verdict. */
  gateFromToolchain(request: ToolchainGateRequest): Promise<ToolchainGateResult>;
}

/** HUP-S6.7 — what CitrateScan says about an address. Mirrors Rust `contract_reader::VerifiedSource`. */
export interface ContractSourceView {
  status: "verified" | "partial" | "unverified" | "notContract";
  isContract: boolean;
  codeSize: number | null;
  contractName: string | null;
  compilerVersion: string | null;
  abi: unknown[] | null;
  source: string | null;
  note: string | null;
}

/** HUP-S6.6 — mirrors Rust `postdeploy::PostDeployStatus`. */
export interface PostDeployStatus {
  contractName: string;
  siteContract: string | null;
  built: boolean;
  exportDir: string | null;
}

/** HUP-S6.6 — mirrors Rust `postdeploy::DeployReceipt`. */
export interface DeployReceiptView {
  txHash: string;
  blockNumber: number;
  status: number | null;
  contractAddress: string | null;
}

/** HUP-S6.6 — mirrors Rust `postdeploy::VerifyOutcome`. */
export interface VerifyOutcomeView {
  status: "verified" | "partial" | "failed" | "unavailable";
  guid: string | null;
  matchType: string | null;
  contractName: string | null;
  message: string;
}

/** HUP-S6.6 — mirrors Rust `postdeploy::SitePin`. */
export interface SitePinView {
  cid: string;
  files: number;
  bytes: number;
  localGatewayUrl: string;
  publicGatewayUrl: string;
  note: string;
}

/** CORE-A3 — mirrors Rust `deploy_records::DeployRecordSummary`. */
export interface DeployRecordSummaryView {
  chainId: number;
  address: string;
  txHash: string;
  blockNumber: number;
  deployer: string;
  initcodeHash: string;
  contractName: string;
  solcVersion: string;
  abiEntries: number;
  standardJsonBytes: number | null;
  standardJsonNote: string | null;
  gateVerdict: "READY" | "NOT_READY";
  gateEvaluatedAtMs: number;
  recordedAtMs: number;
}

/** CORE-A3 — one saved deploy record in full; mirrors Rust `deploy_records::DeployRecord`. */
export interface DeployRecordView {
  chainId: number;
  address: string;
  txHash: string;
  blockNumber: number;
  deployer: string;
  initcodeHash: string;
  contractName: string;
  /** The forge artifact's ABI, as it is. */
  abi: unknown;
  constructorArgsHex: string;
  compiler: DeployGateRecord["compiler"];
  standardJsonInput: string | null;
  standardJsonNote: string | null;
  gate: DeployGateRecord;
  recordedAtMs: number;
}

/** CORE-A3 — mirrors Rust `deploy_records::DeployRecordSaved`. */
export interface DeployRecordSavedView {
  record: DeployRecordSummaryView;
  alreadySaved: boolean;
  count: number;
}

/** HUP-S6.6 — mirrors Rust `postdeploy::VercelExport`. */
export interface VercelExportView {
  dir: string;
  files: number;
  commands: string[];
}
