// L8 bridge impl, TAURI: `bridge.trainTogether` in the desktop app (src/bridge/lanes/l8.ts). Each member
// L8 adds here invokes one registered L8 command (src-tauri/src/commands/l8.rs) through
// `./invoke`. No members yet, so nothing is invoked.
import type { TrainTogetherDomain } from "../lanes/l8";

export const tauriTrainTogether: TrainTogetherDomain = {};
