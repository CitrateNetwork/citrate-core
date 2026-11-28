// L9 bridge impl, TAURI: `bridge.pqc` in the desktop app (src/bridge/lanes/l9.ts). Each member
// L9 adds here invokes one registered L9 command (src-tauri/src/commands/l9.rs) through
// `./invoke`. No members yet, so nothing is invoked.
import type { PqcDomain } from "../lanes/l9";

export const tauriPqc: PqcDomain = {};
