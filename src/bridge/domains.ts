// =====================================================================
// citrate-core — bridge domain contracts (CORE-A1 · A1.1)
//
// One typed interface per domain: auth, wallet, node, memory, chat,
// membership, config (and the CX domains below). The commissary and comms seam
// stubs that nothing called were removed in 0.5.4. Each declares the async functions
// its surface needs — the shapes the current `store.*` methods already imply.
//
// A1 wires exactly ONE domain for real (`config`, the round-trip proof). The
// other eight are typed contracts whose sim impl delegates to the prototype
// Store and whose Tauri impl returns `Unavailable` (Rule 1) until a later
// phase flips it. The surfaces above these interfaces never change.
//
// L0-WP05 (planset v060): each domain now lives in its own file, src/bridge/domains/<name>.ts,
// owned by one lane (.agentile/v60-ownership.map). This file stays the import path every surface
// uses: it re-exports them and keeps the composition types (BridgeContract, CxBridge). Add a
// domain as a new file, one line here and its key below (an L0 PR).
// =====================================================================
import type { FlDatasetsDomain } from "../fl/datasetTypes";
import type { AgentDomain } from "./domains/agent";
import type { AgentHarnessDomain } from "./domains/agentHarness";
import type { AgentSkillsDomain } from "./domains/agentSkills";
import type { AuthDomain } from "./domains/auth";
import type { ChatDomain } from "./domains/chat";
import type { ClusterDomain } from "./domains/cluster";
import type { ComponentsDomain } from "./domains/components";
import type { ConfigDomain } from "./domains/config";
import type { ConnectionsDomain } from "./domains/connections";
import type { ContractsDomain } from "./domains/contracts";
import type { CustodyDomain } from "./domains/custody";
import type { DelegationDomain } from "./domains/delegation";
import type { EscalationDomain } from "./domains/escalation";
import type { FlRoundsDomain } from "./domains/flRounds";
import type { GroupsDomain } from "./domains/groups";
import type { InvitesDomain } from "./domains/invites";
import type { MembershipDomain } from "./domains/membership";
import type { MemoryDomain } from "./domains/memory";
import type { ModelDomain } from "./domains/model";
import type { ModelsCatalogDomain } from "./domains/modelsCatalog";
import type { NodeDomain } from "./domains/node";
import type { ProvidersDomain } from "./domains/providers";
import type { ServicesDomain } from "./domains/services";
import type { ShellDomain } from "./domains/shell";
import type { SigningDomain } from "./domains/signing";
import type { SocialDomain } from "./domains/social";
import type { StorageDomain } from "./domains/storage";
import type { StoreDomain } from "./domains/store";
import type { TelemetryDomain } from "./domains/telemetry";
import type { TierDomain } from "./domains/tier";
import type { TrainingDomain } from "./domains/training";
import type { WalletDomain } from "./domains/wallet";

export * from "./domains/config";
export * from "./domains/custody";
export * from "./domains/auth";
export * from "./domains/signing";
export * from "./domains/wallet";
export * from "./domains/node";
export * from "./domains/model";
export * from "./domains/agent";
export * from "./domains/memory";
export * from "./domains/chat";
export * from "./domains/membership";
export * from "./domains/connections";
export * from "./domains/shell";
export * from "./domains/modelsCatalog";
export * from "./domains/storage";
export * from "./domains/groups";
export * from "./domains/cluster";
export * from "./domains/training";
export * from "./domains/flRounds";
export * from "./domains/agentHarness";
export * from "./domains/agentSkills";
export * from "./domains/social";
export * from "./domains/invites";
export * from "./domains/contracts";
export * from "./domains/telemetry";
export * from "./domains/tier";
export * from "./domains/escalation";
export * from "./domains/components";
export * from "./domains/store";
export * from "./domains/services";
export * from "./domains/providers";
export * from "./domains/delegation";

// ---- the full bridge surface ----------------------------------------
export interface BridgeContract {
  readonly mode: "sim" | "tauri";
  shell: ShellDomain;
  config: ConfigDomain;
  custody: CustodyDomain;
  auth: AuthDomain;
  signing: SigningDomain;
  wallet: WalletDomain;
  node: NodeDomain;
  model: ModelDomain;
  agent: AgentDomain;
  memory: MemoryDomain;
  chat: ChatDomain;
  membership: MembershipDomain;
  connections: ConnectionsDomain;
}

// =====================================================================
// CX — social-node domains (planset citrate-core-social). Frozen in CX-S0.2 and
// composed onto BridgeContract as `bridge: BridgeContract & CxBridge` (bridge/index.ts),
// so the existing monolith bridges are never edited. Each domain's impl lives in its own
// file owned by its lane (src/bridge/{tauri,sim}/<domain>.ts) — see .agentile/cx-ownership.map.
// Interfaces are contract-first (Rule 7): declared here BEFORE any implementation, and only
// changed via a serialized spine-PR (01_SCOPE §5.2).
// =====================================================================

/**
 * The CX domain surface, composed onto the bridge alongside the legacy domains.
 * FROZEN (CX-S0.2): each domain's impl lives in its own lane-owned file; changing a
 * signature/DTO here requires a serialized spine-PR (01_SCOPE §5.2). Every entry is
 * additive — no existing interface above this block is modified.
 */
export interface CxBridge {
  modelsCatalog: ModelsCatalogDomain;
  tier: TierDomain;
  escalation: EscalationDomain;
  telemetry: TelemetryDomain;
  storage: StorageDomain;
  groups: GroupsDomain;
  cluster: ClusterDomain;
  training: TrainingDomain;
  flRounds: FlRoundsDomain;
  /** 0.5.4 WP-H6: the local dataset store (src/fl/datasetTypes.ts). */
  flDatasets: FlDatasetsDomain;
  agentHarness: AgentHarnessDomain;
  agentSkills: AgentSkillsDomain;
  contracts: ContractsDomain;
  social: SocialDomain;
  invites: InvitesDomain;
  components: ComponentsDomain;
  store: StoreDomain;
  services: ServicesDomain;
  providers: ProvidersDomain;
  delegation: DelegationDomain;
}
