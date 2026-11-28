// L4 bridge namespace `bridge.conversations` (planset v060, lanes/L0_INTEGRATION_SEAMS.md section 3.3):
// sealed group history, pins and roster names. L4 owns this file and its two impls, src/bridge/tauri/l4.ts and
// src/bridge/sim/l4.ts (.agentile/v60-ownership.map); no other lane edits or imports them.
//
// It starts with no members. Each member L4 adds names its data source (Rule 7) and lands with
// both impls: the tauri one invokes a registered L4 command, the sim one answers honestly or throws
// `Unavailable` (src/bridge/types.ts), never an invented value (Rule 1). Types only in this file.
export interface ConversationsDomain {}
