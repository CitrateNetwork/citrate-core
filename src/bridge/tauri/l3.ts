// L3 bridge impl, TAURI: `bridge.notify` in the desktop app (src/bridge/lanes/l3.ts). Each member
// L3 adds here invokes one registered L3 command (src-tauri/src/commands/l3.rs) through
// `./invoke`. No members yet, so nothing is invoked.
import type { NotifyDomain } from "../lanes/l3";

export const tauriNotify: NotifyDomain = {};
