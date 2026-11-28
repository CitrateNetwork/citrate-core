// L9 bridge namespace `bridge.pqc` (planset v060, lanes/L0_INTEGRATION_SEAMS.md section 3.3):
// the PQC inventory and the envelope status of each store. L9 owns this file and its two impls, src/bridge/tauri/l9.ts and
// src/bridge/sim/l9.ts (.agentile/v60-ownership.map); no other lane edits or imports them.
//
// It starts with no members. Each member L9 adds names its data source (Rule 7) and lands with
// both impls: the tauri one invokes a registered L9 command, the sim one answers honestly or throws
// `Unavailable` (src/bridge/types.ts), never an invented value (Rule 1). Types only in this file.
export interface PqcDomain {}
