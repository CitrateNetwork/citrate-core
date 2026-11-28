// L5 bridge impl, TAURI: `bridge.media` in the desktop app (src/bridge/lanes/l5.ts). Each member
// L5 adds here invokes one registered L5 command (src-tauri/src/commands/l5.rs) through
// `./invoke`. No members yet, so nothing is invoked.
import type { MediaDomain } from "../lanes/l5";

export const tauriMedia: MediaDomain = {};
