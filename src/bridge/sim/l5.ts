// L5 bridge impl, SIM (web/dev): `bridge.media` in the browser preview (src/bridge/lanes/l5.ts).
// The preview has no backend for L5, so a member added here throws `Unavailable`
// (src/bridge/types.ts) unless the prototype Store holds an honest answer (Rule 1). No members yet.
import type { MediaDomain } from "../lanes/l5";
import type { SimHost } from "./index";

export function simMedia(_host: SimHost): MediaDomain {
  return {};
}
