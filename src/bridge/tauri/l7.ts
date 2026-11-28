// L7 bridge impl, TAURI: `bridge.net` in the desktop app (src/bridge/lanes/l7.ts). Each member
// L7 adds here invokes one registered L7 command (src-tauri/src/commands/l7.rs) through
// `./invoke`. No members yet, so nothing is invoked.
import type { NetDomain } from "../lanes/l7";

export const tauriNet: NetDomain = {};
