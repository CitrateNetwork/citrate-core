// L7 bridge impl, SIM (web/dev): `bridge.net` in the browser preview (src/bridge/lanes/l7.ts).
// The preview has no backend for L7, so a member added here throws `Unavailable`
// (src/bridge/types.ts) unless the prototype Store holds an honest answer (Rule 1). No members yet.
import type { NetDomain } from "../lanes/l7";
import type { SimHost } from "./index";

export function simNet(_host: SimHost): NetDomain {
  return {};
}
