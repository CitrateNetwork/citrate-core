// =====================================================================
// citrate-core — CX bridge composition (planset citrate-core-social, 02_ARCHITECTURE §3)
//
// Composes the per-domain CX bridge modules — each owned by its lane (s1..s6) in its own
// file (src/bridge/{tauri,sim}/<domain>.ts) — onto the `CxBridge` surface, which `./index.ts`
// spreads onto the bridge as `bridge: BridgeContract & CxBridge`. A CX feature lane fills in
// its OWN domain file; this barrel (frozen in S0) wires it in. Adding a lane = one additive,
// non-conflicting line here (planset 01_SCOPE §4-§5).
// =====================================================================
import type { CxBridge } from "./domains";
import type { SimHost } from "./sim";

import { tauriModelsCatalog } from "./tauri/models";
import { tauriTier } from "./tauri/tier";
import { tauriEscalation } from "./tauri/escalation";
import { tauriTelemetry } from "./tauri/telemetry";
import { tauriStorage } from "./tauri/storage";
import { tauriGroups } from "./tauri/comms";
import { tauriCluster } from "./tauri/cluster";
import { tauriTraining } from "./tauri/training";
import { tauriFlRounds } from "./tauri/flRounds";
import { tauriAgentHarness, tauriAgentSkills } from "./tauri/agent";
import { tauriContracts } from "./tauri/contracts";
import { tauriSocial } from "./tauri/social";
import { tauriInvites } from "./tauri/invites";
import { tauriComponents } from "./tauri/components";

import { simModelsCatalog } from "./sim/models";
import { simTier } from "./sim/tier";
import { simEscalation } from "./sim/escalation";
import { simTelemetry } from "./sim/telemetry";
import { simStorage } from "./sim/storage";
import { simGroups } from "./sim/comms";
import { simCluster } from "./sim/cluster";
import { simTraining } from "./sim/training";
import { simFlRounds } from "./sim/flRounds";
import { simAgentHarness, simAgentSkills } from "./sim/agent";
import { simContracts } from "./sim/contracts";
import { simSocial } from "./sim/social";
import { simInvites } from "./sim/invites";
import { simComponents } from "./sim/components";

/** CX domains, tauri (real) side. */
export function cxTauri(): CxBridge {
  return {
    modelsCatalog: tauriModelsCatalog,
    tier: tauriTier,
    escalation: tauriEscalation,
    telemetry: tauriTelemetry,
    storage: tauriStorage,
    groups: tauriGroups,
    cluster: tauriCluster,
    training: tauriTraining,
    flRounds: tauriFlRounds,
    agentHarness: tauriAgentHarness,
    agentSkills: tauriAgentSkills,
    contracts: tauriContracts,
    social: tauriSocial,
    invites: tauriInvites,
    components: tauriComponents,
  };
}

/** CX domains, sim (web/dev) side. */
export function cxSim(host: SimHost): CxBridge {
  return {
    modelsCatalog: simModelsCatalog(host),
    tier: simTier(host),
    escalation: simEscalation(host),
    telemetry: simTelemetry(host),
    storage: simStorage(host),
    groups: simGroups(host),
    cluster: simCluster(host),
    training: simTraining(host),
    flRounds: simFlRounds(host),
    agentHarness: simAgentHarness(host),
    agentSkills: simAgentSkills(host),
    contracts: simContracts(host),
    social: simSocial(host),
    invites: simInvites(host),
    components: simComponents(host),
  };
}
