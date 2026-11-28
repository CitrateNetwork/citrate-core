// L4 bridge impl, SIM (web/dev): `bridge.conversations` in the browser preview (src/bridge/lanes/l4.ts).
// The preview has no backend for L4, so a member added here throws `Unavailable`
// (src/bridge/types.ts) unless the prototype Store holds an honest answer (Rule 1). No members yet.
import type { ConversationsDomain } from "../lanes/l4";
import type { SimHost } from "./index";

export function simConversations(_host: SimHost): ConversationsDomain {
  return {};
}
