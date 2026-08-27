// =====================================================================
// citrate-core — the BRIDGE (CORE-A1)
//
// The one seam between the surfaces and the backend. `bridge.mode` is
// runtime-selected once, at the boundary: `tauri` in the packaged app,
// `sim` on web/dev. Every later beta phase flips ONE domain's implementation
// sim→live; the surfaces above this seam never change.
//
//   import { bridge } from "./bridge";
//   const cfg = await bridge.config.read();   // real in Tauri, sim on web
//
// The sim adapter delegates to the prototype Store; to avoid a store↔bridge
// import cycle the Store binds itself here once via `bindSimHost()`.
// =====================================================================
import { BRIDGE_MODE } from "./mode";
import type { BridgeContract, CxBridge } from "./domains";
import { createSimBridge, type SimHost } from "./sim";
import { createTauriBridge } from "./tauri";
import { cxTauri, cxSim } from "./cx";
import type { AppState } from "../shell/state";
import { DEFAULT_APP_CONFIG } from "./types";

export type { BridgeContract, CxBridge } from "./domains";
/** The full bridge = the legacy contract + the CX social-node domains (planset). */
export type FullBridge = BridgeContract & CxBridge;
export type { AppConfig, KeyringStatus, CustodyStatus, SlotInfo } from "./types";
export { Unavailable, isUnavailable } from "./types";

// --- sim host binding (dev only) -------------------------------------
// A default host so the sim bridge is usable before the Store binds (e.g. in
// unit tests). The real Store overrides this in its constructor.
let simHost: SimHost = {
  getState: () =>
    ({
      ...DEFAULT_APP_CONFIG,
      liquid: 0,
      selfStake: 0,
      hasGrant: false,
      claimable: 0,
      walletAddr: "0x",
      activity: [],
      node: "off",
      peers: 0,
      height: 0,
      syncPct: 0,
      chatBackend: "gateway",
      entitlement: "active",
      tier: "free",
      org: null,
      connections: {},
    }) as unknown as AppState,
  patch: () => {},
};

/** The Store calls this once, in its constructor, to bind the sim adapter. */
export function bindSimHost(host: SimHost): void {
  simHost = host;
}

// --- assembly --------------------------------------------------------
// The legacy monolith bridges (createTauriBridge/createSimBridge) are spread unchanged;
// the CX domains (cxTauri/cxSim) are spread alongside, so adding a CX feature never edits
// the monolith — the parallel-safe seam (planset 02_ARCHITECTURE §3).
function assemble(): FullBridge {
  if (BRIDGE_MODE === "tauri") {
    return { mode: BRIDGE_MODE, ...createTauriBridge(), ...cxTauri() };
  }
  // In sim mode we read through a live getter so the Store can bind late.
  const host: SimHost = {
    getState: () => simHost.getState(),
    patch: (u) => simHost.patch(u),
  };
  return { mode: BRIDGE_MODE, ...createSimBridge(host), ...cxSim(host) };
}

export const bridge: FullBridge = assemble();
