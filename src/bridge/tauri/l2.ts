// L2 bridge impl, TAURI: `bridge.hic` in the desktop app (src/bridge/lanes/l2.ts). Each member
// L2 adds here invokes one registered L2 command (src-tauri/src/commands/l2.rs) through
// `./invoke`. No members yet, so nothing is invoked.
import type { HicDomain } from "../lanes/l2";

export const tauriHic: HicDomain = {};
