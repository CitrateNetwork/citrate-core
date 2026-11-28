// L2 bridge impl, SIM (web/dev): `bridge.hic` in the browser preview (src/bridge/lanes/l2.ts).
// The preview has no backend for L2, so a member added here throws `Unavailable`
// (src/bridge/types.ts) unless the prototype Store holds an honest answer (Rule 1). No members yet.
import type { HicDomain } from "../lanes/l2";
import type { SimHost } from "./index";

export function simHic(_host: SimHost): HicDomain {
  return {};
}
