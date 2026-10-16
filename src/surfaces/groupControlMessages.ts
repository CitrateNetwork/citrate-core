// Control messages ride the group relay next to chat and are never shown in the conversation:
// `cbind1:` verified social bindings (ADR-2026-08-30 D1) and `cdlink1:` DeviceLink shares (HUP-S8.1).
import { DEVICE_LINKS_MSG_PREFIX, SOCIAL_BINDING_MSG_PREFIX } from "../bridge/domains";

const CONTROL_PREFIXES = [SOCIAL_BINDING_MSG_PREFIX, DEVICE_LINKS_MSG_PREFIX];

export const isControlMessage = (body: string): boolean => CONTROL_PREFIXES.some((p) => body.startsWith(p));

/** The messages a member reads: everything except control messages. */
export function visibleConversation<M extends { body: string }>(messages: M[]): M[] {
  return messages.filter((m) => !isControlMessage(m.body));
}
