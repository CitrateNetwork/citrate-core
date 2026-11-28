// L4 bridge impl, TAURI: `bridge.conversations` in the desktop app (src/bridge/lanes/l4.ts). Each member
// L4 adds here invokes one registered L4 command (src-tauri/src/commands/l4.rs) through
// `./invoke`. No members yet, so nothing is invoked.
import type { ConversationsDomain } from "../lanes/l4";

export const tauriConversations: ConversationsDomain = {};
