// L5 bridge namespace `bridge.media` (planset v060, lanes/L0_INTEGRATION_SEAMS.md section 3.3):
// media capabilities, image generation, speech to text, text to speech and LoRA teach. L5 owns this file and its two impls, src/bridge/tauri/l5.ts and
// src/bridge/sim/l5.ts (.agentile/v60-ownership.map); no other lane edits or imports them.
//
// It starts with no members. Each member L5 adds names its data source (Rule 7) and lands with
// both impls: the tauri one invokes a registered L5 command, the sim one answers honestly or throws
// `Unavailable` (src/bridge/types.ts), never an invented value (Rule 1). Types only in this file.
export interface MediaDomain {}
