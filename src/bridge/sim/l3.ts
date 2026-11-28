// L3 bridge impl, SIM (web/dev): `bridge.notify` in the browser preview (src/bridge/lanes/l3.ts).
// The preview has no backend for L3, so a member added here throws `Unavailable`
// (src/bridge/types.ts) unless the prototype Store holds an honest answer (Rule 1). No members yet.
import type { NotifyDomain } from "../lanes/l3";
import type { SimHost } from "./index";

export function simNotify(_host: SimHost): NotifyDomain {
  return {};
}
