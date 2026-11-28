// L7 bridge namespace `bridge.net` (planset v060, lanes/L0_INTEGRATION_SEAMS.md section 3.3):
// the tailnet, Cloudflare, device pairing and mesh state. L7 owns this file and its two impls, src/bridge/tauri/l7.ts and
// src/bridge/sim/l7.ts (.agentile/v60-ownership.map); no other lane edits or imports them.
//
// It starts with no members. Each member L7 adds names its data source (Rule 7) and lands with
// both impls: the tauri one invokes a registered L7 command, the sim one answers honestly or throws
// `Unavailable` (src/bridge/types.ts), never an invented value (Rule 1). Types only in this file.
export interface NetDomain {}
