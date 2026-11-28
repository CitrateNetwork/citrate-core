// L1 bridge impl, SIM (web/dev): `bridge.acp` in the browser preview (src/bridge/lanes/l1.ts).
// The preview has no backend for L1, so a member added here throws `Unavailable`
// (src/bridge/types.ts) unless the prototype Store holds an honest answer (Rule 1). No members yet.
import type { AcpDomain } from "../lanes/l1";
import type { SimHost } from "./index";

export function simAcp(_host: SimHost): AcpDomain {
  return {};
}
