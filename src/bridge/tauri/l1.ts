// L1 bridge impl, TAURI: `bridge.acp` in the desktop app (src/bridge/lanes/l1.ts). Each member
// L1 adds here invokes one registered L1 command (src-tauri/src/commands/l1.rs) through
// `./invoke`. No members yet, so nothing is invoked.
import type { AcpDomain } from "../lanes/l1";

export const tauriAcp: AcpDomain = {};
