// GROW-S1 task 11 — the inviter's two links, as plain async steps over the bridge (tested with fakes).
//
//   Direct invite: the relay's one-time self-admit invite (`invites.create`), sealed in Rust into a
//   short link `https://citrate.ai/join/<code>#K` (`join.mint(direct)`). The service stores only
//   ciphertext; the key rides after `#`, which browsers never send to a server.
//   Public link: a referral code (`join.mint(referral)`), display only: it shows who shared it and
//   the group's name, and carries no group id and no wallet address.
//
// Data sources (Rule 7): the comms relay (`group_invite_create` / `group_invite_revoke`) and the
// citrate.ai join service through the Rust join client (`join_mint` / `join_attach` / `join_revoke`).
import type { InvitesDomain, JoinCodeEntry, JoinDomain, JoinMinted } from "../bridge/domains";
import { joinErrorKind, joinErrorMessage, type JoinGoalId } from "./referral";

export interface InviteDeps {
  invites: Pick<InvitesDomain, "create" | "revoke">;
  join: Pick<JoinDomain, "mint" | "attach" | "revoke">;
}

export type DirectOutcome =
  /** The short link is ready. */
  | { kind: "link"; minted: JoinMinted; relayToken: string }
  /** The code is reserved but its sealed invite isn't attached yet: retry `attach`. */
  | { kind: "pending"; minted: JoinMinted; relayToken: string }
  /** The short-link service is unavailable: the app-only relay link is offered instead. */
  | { kind: "fallback"; relayLink: string; relayToken: string }
  | { kind: "error"; message: string };

export type PublicOutcome = { kind: "link"; minted: JoinMinted } | { kind: "error"; message: string };

/** The display name the service gets: trimmed and clipped to its 60-character limit. */
export function inviteName(name: string): string {
  return name.trim().slice(0, 60);
}

/** Create a direct invite for a group. */
export async function createDirectInvite(d: InviteDeps, groupId: string, groupName: string, goal: JoinGoalId): Promise<DirectOutcome> {
  let relay: { token: string; link: string };
  try {
    relay = await d.invites.create(groupId, "shared link");
  } catch (e) {
    return { kind: "error", message: "Couldn't create an invite: " + (e instanceof Error ? e.message : String(e)) };
  }
  try {
    const minted = await d.join.mint({ kind: "direct", clusterName: inviteName(groupName), goal }, relay.link);
    if (minted.status === "valid" && minted.shareUrl) return { kind: "link", minted, relayToken: relay.token };
    return { kind: "pending", minted, relayToken: relay.token };
  } catch (e) {
    if (joinErrorKind(e) === "unavailable") return { kind: "fallback", relayLink: relay.link, relayToken: relay.token };
    // No short link will carry this relay invite: revoke it so no unused one-time invite lingers.
    await d.invites.revoke(groupId, relay.token).catch(() => {});
    return { kind: "error", message: joinErrorMessage(e) };
  }
}

/** Retry attaching a pending direct code. */
export async function retryDirectInvite(d: InviteDeps, code: string, relayToken: string): Promise<DirectOutcome> {
  try {
    const minted = await d.join.attach(code);
    if (minted.status === "valid" && minted.shareUrl) return { kind: "link", minted, relayToken };
    return { kind: "pending", minted, relayToken };
  } catch (e) {
    return { kind: "error", message: joinErrorMessage(e) };
  }
}

/** Create a public (referral) link. There is no fallback to the older self-built format: links the
 *  app builds now carry only display data. */
export async function createPublicLink(d: InviteDeps, name: string, goal: JoinGoalId): Promise<PublicOutcome> {
  try {
    const minted = await d.join.mint({ kind: "referral", clusterName: inviteName(name), goal });
    if (!minted.shareUrl) return { kind: "error", message: "The invite service didn't return a link. Try again." };
    return { kind: "link", minted };
  } catch (e) {
    return { kind: "error", message: joinErrorKind(e) === "unavailable" ? "Public links are not available right now. Try again later." : joinErrorMessage(e) };
  }
}

/** Revoke a direct invite: the short code at the service and the relay invite sealed inside it. */
export async function revokeDirectInvite(d: InviteDeps, groupId: string, code: string | null, relayToken: string): Promise<void> {
  const jobs: Promise<unknown>[] = [d.invites.revoke(groupId, relayToken)];
  if (code) jobs.unshift(d.join.revoke(code));
  const results = await Promise.allSettled(jobs);
  const failed = results.find((r) => r.status === "rejected") as PromiseRejectedResult | undefined;
  if (failed) throw failed.reason instanceof Error ? failed.reason : new Error(String(failed.reason));
}

// --- The member's own codes, recovered from the service (`join.list`) ----------------------------
// The inviter's panel keeps a fresh link only in component state, so after the view unmounts the
// member's codes come back from `GET /api/join/mine`. A pending direct code can be finished while
// this app run still holds its sealed envelope (Rust memory); any live code can be revoked.

export type CodeAction = "finish" | "revoke";

/** What the member can do with one of their codes. */
export function codeActions(_e: JoinCodeEntry): CodeAction[] {
  // RED stub: the old behaviour (no recovered actions).
  return [];
}

export type FinishOutcome = { kind: "link"; minted: JoinMinted } | { kind: "pending"; minted: JoinMinted } | { kind: "error"; message: string };

/** Finish a pending direct code (attach its sealed invite). */
export async function finishPendingCode(_d: Pick<InviteDeps, "join">, _code: string): Promise<FinishOutcome> {
  // RED stub.
  return { kind: "error", message: "" };
}

/** Revoke one of the member's codes at the service. */
export async function revokeCode(_d: Pick<InviteDeps, "join">, _code: string): Promise<void> {
  // RED stub.
}

/** "Expires 1 Nov 2026" from the service's ISO expiry; "—" when it can't be read (never a guess). */
export function expiryLabel(iso: string): string {
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return "Expiry —";
  return "Expires " + new Date(t).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}
