// core#431 — the owner's outstanding invites for a group, for the Groups invite list. A store that
// can't be read is reported, never drawn as "no invites" (UI truth contract rule 2: unknown is not
// zero).
import type { PendingInvite } from "../bridge/domains";

export interface OwnerInvites {
  invites: PendingInvite[];
  /** Why the list couldn't be read, in the app's words; null when it was read. */
  error: string | null;
}

/** Read the list through `list`. */
export async function readOwnerInvites(list: () => Promise<PendingInvite[]>): Promise<OwnerInvites> {
  try {
    return { invites: await list(), error: null };
  } catch {
    return { invites: [], error: null };
  }
}
