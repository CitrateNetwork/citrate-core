// L8 bridge impl, SIM (web/dev): `bridge.trainTogether` in the browser preview (src/bridge/lanes/l8.ts).
// The preview has no backend for L8, so a member added here throws `Unavailable`
// (src/bridge/types.ts) unless the prototype Store holds an honest answer (Rule 1). No members yet.
import type { TrainTogetherDomain } from "../lanes/l8";
import type { SimHost } from "./index";

export function simTrainTogether(_host: SimHost): TrainTogetherDomain {
  return {};
}
