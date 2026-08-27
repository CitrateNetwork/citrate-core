// =====================================================================
// citrate-core — CX bridge composition (planset citrate-core-social, 02_ARCHITECTURE §3)
//
// Composes the per-domain CX bridge modules — each owned by its lane (s1..s6) in its own
// file (src/bridge/{tauri,sim}/<domain>.ts) — onto the `CxBridge` surface, which `./index.ts`
// spreads onto the bridge as `bridge: BridgeContract & CxBridge`. This is why a CX feature
// lane never edits the existing monolith bridges: it fills in its OWN domain file, and this
// barrel (frozen in S0) wires it in.
//
// S0.2 wires the worked example (modelsCatalog). Later lanes add their line here — an
// additive, non-conflicting entry per domain (planset 01_SCOPE §4-§5).
// =====================================================================
import type { CxBridge } from "./domains";
import type { SimHost } from "./sim";
import { tauriModelsCatalog } from "./tauri/models";
import { simModelsCatalog } from "./sim/models";

/** CX domains, tauri (real) side. */
export function cxTauri(): CxBridge {
  return {
    modelsCatalog: tauriModelsCatalog,
  };
}

/** CX domains, sim (web/dev) side. */
export function cxSim(host: SimHost): CxBridge {
  return {
    modelsCatalog: simModelsCatalog(host),
  };
}
