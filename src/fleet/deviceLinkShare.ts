// =====================================================================
// citrate-core — DeviceLink sharing over the group relay (HUP-S8.1 follow-on)
//
// Another member's node admits your linked machine only if it knows the machine's signed link. Each
// member shares its own links and revocations with the groups it is in, as a control message the
// chat hides (prefix U+0001 `cdlink1:`, like the social bindings' U+0001 `cbind1:`). Core builds the message (only
// when this member has something to share and has not sent this exact set to that group) and
// verifies every incoming one: the relay must attribute it to the member it speaks for, and every
// signature must recover. These helpers only move bytes; they never decide trust.
// =====================================================================
import { DEVICE_LINKS_MSG_PREFIX, type DeviceLinkIngest, type DeviceLinkShareOffer } from "../bridge/domains";

export const isDeviceLinkMessage = (body: string): boolean => body.startsWith(DEVICE_LINKS_MSG_PREFIX);

export interface ShareDeps {
  offer(groupId: string): Promise<DeviceLinkShareOffer | null>;
  mark(groupId: string, digest: string): Promise<void>;
  send(groupId: string, body: string): Promise<void>;
}

/** Send each group the current share (if any) and mark it sent. Best effort: returns how many groups
 *  were sent to; a failed send is not marked, so the next call retries it. */
export async function shareDeviceLinks(groupIds: string[], deps: ShareDeps): Promise<number> {
  let sent = 0;
  for (const g of groupIds) {
    try {
      const o = await deps.offer(g);
      if (!o) continue;
      await deps.send(g, o.body);
      await deps.mark(g, o.digest);
      sent += 1;
    } catch {
      /* best effort per group: relay down, daemon not running */
    }
  }
  return sent;
}

interface Msg {
  sender: string;
  body: string;
}

/** The newest share message of each sender (a share carries that member's full current set). */
export function latestPerSender<M extends Msg>(msgs: M[]): M[] {
  const last = new Map<string, M>();
  for (const m of msgs) if (isDeviceLinkMessage(m.body)) last.set(m.sender.toLowerCase(), m);
  return [...last.values()];
}

/** Hand the newest share of each sender to core. Never throws (a refused share is not an error the
 *  member can act on); returns what core accepted. */
export async function ingestDeviceLinkMessages(
  msgs: Msg[],
  ingest: (sender: string, body: string) => Promise<DeviceLinkIngest>,
): Promise<{ links: number; revocations: number }> {
  const total = { links: 0, revocations: 0 };
  for (const m of latestPerSender(msgs)) {
    try {
      const r = await ingest(m.sender, m.body);
      total.links += r.links;
      total.revocations += r.revocations;
    } catch {
      /* refused in core: malformed, too large, or not from its member */
    }
  }
  return total;
}
