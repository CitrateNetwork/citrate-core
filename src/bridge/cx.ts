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
import { tauriStorage } from "./tauri/storage";
import { tauriGroups } from "./tauri/comms";
import { tauriCluster } from "./tauri/cluster";
import { tauriTraining } from "./tauri/training";
import { tauriAgentHarness } from "./tauri/agent";

import { simModelsCatalog } from "./sim/models";
import { simStorage } from "./sim/storage";
import { simGroups } from "./sim/comms";
import { simCluster } from "./sim/cluster";
import { simTraining } from "./sim/training";
import { simAgentHarness } from "./sim/agent";

/** CX domains, tauri (real) side. */
export function cxTauri(): CxBridge {
  return {
    modelsCatalog: tauriModelsCatalog,
    storage: tauriStorage,
    groups: tauriGroups,
    cluster: tauriCluster,
    training: tauriTraining,
    agentHarness: tauriAgentHarness,
  };
}

/** CX domains, sim (web/dev) side. */
export function cxSim(host: SimHost): CxBridge {
  return {
    modelsCatalog: simModelsCatalog(host),
    storage: simStorage(host),
    groups: simGroups(host),
    cluster: simCluster(host),
    training: simTraining(host),
    agentHarness: simAgentHarness(host),
  };
}
