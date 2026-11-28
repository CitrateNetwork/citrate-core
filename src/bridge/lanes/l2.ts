// L2 bridge namespace `bridge.hic` (planset v060, lanes/L0_INTEGRATION_SEAMS.md section 3.3):
// the HIC level (get and set), the session budget and sandbox status. L2 owns this file and its two impls, src/bridge/tauri/l2.ts and
// src/bridge/sim/l2.ts (.agentile/v60-ownership.map); no other lane edits or imports them.
//
// It starts with no members. Each member L2 adds names its data source (Rule 7) and lands with
// both impls: the tauri one invokes a registered L2 command, the sim one answers honestly or throws
// `Unavailable` (src/bridge/types.ts), never an invented value (Rule 1). Types only in this file.
export interface HicDomain {}
