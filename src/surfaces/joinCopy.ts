// GROW-S2 (Join 9, task M5) — state → copy for the relay rendezvous join.
//
// The words come from the GROW-S2 contract §11 (citrate-federation
// .agentile/planset/2026-10-19-betanet/JOIN_GROW_S2_RELAY.md, rev 8). They are proposed for the C1 UI
// truth contract §1 (task R7); until R7 lands, §11 is their source, so this file follows it as
// written. Each word has one condition: a `JoinView` / `InviteView` state from the member daemon.
//
// Pure functions only (no React, no bridge), so every state → copy pair is unit-tested.
import type { InviteViewDto, JoinViewDto } from "../bridge/domains";

export type CopyTone = "info" | "ok" | "warn" | "muted";

/** What one join or invite row says, and which actions it offers. */
export interface StateCopy {
  title: string;
  sub?: string;
  /** "last known: …" context for an unreachable relay (never an expired or failed word). */
  lastKnown?: string;
  tone: CopyTone;
  /** Offer Retry (runs a tick / re-check). */
  retry?: boolean;
  /** Offer Cancel (invitee withdraws a pending request). */
  cancel?: boolean;
  /** Offer Dismiss (invitee forgets a terminal join). */
  forget?: boolean;
  /** Offer Remove (inviter offboards a joiner who never picked up the Welcome). */
  remove?: boolean;
}

/** Context the join words need that the view doesn't carry. */
export interface JoinCopyContext {
  /** The cluster's display name (the view's `name`, or the signed name from the invite). */
  cluster: string;
  /** The inviter's signed display name from GROW-S1, when known. */
  inviter?: string;
  /** Whether the group is in the daemon's group list (`listGroups`). "Joined" needs it. */
  groupListed: boolean;
  /** The last view of this join that wasn't `relay_unreachable`, for the "last known" line. */
  lastKnown?: JoinViewDto;
  /** Date formatter (injected so tests are locale-free). */
  fmtDate: (ms: number) => string;
  /** Time formatter for the "last known (⟨time⟩)" line. */
  fmtTime: (ms: number) => string;
}

/** §11 fallback when the inviter's signed name isn't known. */
export const INVITER_FALLBACK = "the person who invited you";

/** Shorten an address the way §11 shows it: `0x12…ab`. Never resolved to a handle. */
export function shortJoiner(addr: string | null | undefined): string {
  const a = (addr ?? "").trim();
  if (!a) return "";
  const body = a.startsWith("0x") || a.startsWith("0X") ? a.slice(2) : a;
  if (body.length <= 4) return `0x${body}`;
  return `0x${body.slice(0, 2)}…${body.slice(-2)}`;
}

/** Envelope opened (GROW-S1 `ResolvedView.actionable`), no `JoinView` for the group yet. */
export function readyToJoinCopy(cluster: string): StateCopy {
  return { title: `Ready to join ${cluster}`, tone: "info" };
}

/** `failed` reasons that are final for the request: no Retry, ask for a new invite (§11, rev 6–7). */
const FINAL_JOIN_FAILURES = new Set([
  "group_mismatch",
  "binding_rejected",
  "domain_mismatch",
  "too_large",
  "quota_exceeded",
  "already_delivered",
]);

function failedReasonText(reason: string | null): string {
  if (!reason) return "";
  if (FINAL_JOIN_FAILURES.has(reason)) return "ask for a new invite";
  if (reason === "mls") return "MLS join error";
  return reason.replace(/_/g, " ");
}

/** The short word used in "last known: …" (§11 example: "Waiting…"). */
function lastKnownWord(v: JoinViewDto, ctx: JoinCopyContext): string | undefined {
  switch (v.state) {
    case "requesting":
      return "Asking to join…";
    case "waiting":
      return "Waiting…";
    case "joining":
      return `Joining ${ctx.cluster}…`;
    case "reply_unverified":
      return "The reply to your invite couldn't be verified";
    default:
      // A terminal last state is never relayed here: an unreachable relay never reads as
      // expired, failed or done.
      return undefined;
  }
}

/** Invitee: one `JoinView` → its §11 copy. */
export function joinCopy(v: JoinViewDto, ctx: JoinCopyContext): StateCopy {
  const inviter = ctx.inviter?.trim() || INVITER_FALLBACK;
  switch (v.state) {
    case "requesting":
      return { title: "Asking to join…", tone: "info", cancel: true };
    case "waiting":
      return {
        title: `Waiting for ${inviter} to let you in${v.graceUntil ? ` · until ${ctx.fmtDate(v.graceUntil)}` : ""}`,
        sub: "Their Citrate adds you the next time it's open. You can close Citrate.",
        tone: "info",
        cancel: true,
      };
    case "joining":
      return { title: `Joining ${ctx.cluster}…`, tone: "info" };
    case "joined":
      // Only when the group is in the daemon's group list. No optimistic "Joined".
      return ctx.groupListed
        ? { title: `Joined ${ctx.cluster}`, tone: "ok", forget: true }
        : { title: `Joining ${ctx.cluster}…`, tone: "info" };
    case "expired":
      return { title: "This invite has expired", tone: "muted", forget: true };
    case "welcome_expired":
      return { title: "This invite wasn't picked up in time. Ask for a new one.", tone: "muted", forget: true };
    case "revoked":
      return { title: "This invite was withdrawn", tone: "muted", forget: true };
    case "taken":
      // Rev 8: an earlier join of this wallet was admitted and the group is on this device.
      return ctx.groupListed
        ? { title: `Joined ${ctx.cluster}`, tone: "ok", forget: true }
        : { title: "Someone else already used this invite", tone: "muted", forget: true };
    case "dropped": {
      if (v.reason === "already_member") return { title: "Already a member", tone: "ok", forget: true };
      const who = ctx.inviter?.trim() ? `${ctx.inviter.trim()}'s app` : "Your inviter's app";
      const why =
        v.reason === "bad_binding"
          ? "your device identity didn't verify"
          : v.reason === "bad_mac" || v.reason === "bad_key_package"
            ? "the request didn't verify; ask for a new invite"
            : "";
      return { title: `${who} couldn't add you${why ? ` · ${why}` : ""}`, tone: "warn", forget: true };
    }
    case "withdrawn":
      return { title: "You cancelled this request", tone: "muted", forget: true };
    case "unknown":
      // Reasons `no_rendezvous` or `purged`. Not "expired".
      return { title: "We couldn't find that invite", tone: "muted", forget: true };
    case "relay_unreachable": {
      const lk = ctx.lastKnown ? lastKnownWord(ctx.lastKnown, ctx) : undefined;
      const at = ctx.lastKnown?.updatedAt ? ` (${ctx.fmtTime(ctx.lastKnown.updatedAt)})` : "";
      return {
        title: "Couldn't check your invite · relay unreachable",
        lastKnown: lk ? `last known: ${lk}${at}` : undefined,
        tone: "warn",
        retry: true,
      };
    }
    case "relay_unsupported":
      return { title: "Invites need an update on our side", tone: "muted" };
    case "reply_unverified":
      return { title: "The reply to your invite couldn't be verified", tone: "warn" };
    case "failed": {
      const final = !!v.reason && FINAL_JOIN_FAILURES.has(v.reason);
      const why = failedReasonText(v.reason);
      return {
        title: `Couldn't finish joining${why ? ` · ${why}` : ""}`,
        tone: "warn",
        retry: !final,
        forget: final,
      };
    }
  }
}

/** Context for the inviter's invites list. */
export interface InviteCopyContext {
  fmtDate: (ms: number) => string;
  fmtTime: (ms: number) => string;
  /** The last view of this invite that wasn't `relay_unreachable`. */
  lastKnown?: InviteViewDto;
}

function inviteFailedReason(reason: string | null): string {
  switch (reason) {
    case null:
    case "":
      return "";
    case "too_large":
      return "the group is too big for an invite-link admit today";
    case "not_a_member":
      return "you are no longer in this group";
    case "rate_limited":
      return "rate limited";
    case "quota_exceeded":
      return "quota exceeded";
    case "mls":
      return "MLS error";
    default:
      return reason.replace(/_/g, " ");
  }
}

function inviteLastKnownWord(v: InviteViewDto, ctx: InviteCopyContext): string | undefined {
  switch (v.state) {
    case "open":
      return `Not used yet · expires ${ctx.fmtDate(v.expiresAt)}`;
    case "adding":
      return v.joiner ? `Adding ${shortJoiner(v.joiner)}…` : "Adding…";
    case "added":
      return v.joiner ? `Added ${shortJoiner(v.joiner)} · waiting for them to open Citrate` : undefined;
    default:
      return undefined;
  }
}

/** Inviter: one `InviteView` → its §11 copy. */
export function inviteCopy(v: InviteViewDto, ctx: InviteCopyContext): StateCopy {
  const who = shortJoiner(v.joiner);
  switch (v.state) {
    case "open":
      return { title: `Not used yet · expires ${ctx.fmtDate(v.expiresAt)}`, tone: "info" };
    case "adding":
      return { title: who ? `Adding ${who}…` : "Adding…", tone: "info" };
    case "added":
      return { title: `Added ${who} · waiting for them to open Citrate`, tone: "info" };
    case "picked_up":
      return { title: `Picked up by ${who}`, tone: "ok" };
    case "welcome_expired":
      return { title: `Not picked up · Remove ${who}?`, tone: "warn", remove: !!v.joiner };
    case "expired":
      return { title: "Expired", tone: "muted" };
    case "revoked":
      return { title: "Withdrawn", tone: "muted" };
    case "failed": {
      const why = inviteFailedReason(v.reason);
      return { title: `Couldn't add${why ? ` · ${why}` : ""}`, tone: "warn" };
    }
    case "relay_unreachable": {
      const lk = ctx.lastKnown ? inviteLastKnownWord(ctx.lastKnown, ctx) : undefined;
      const at = ctx.lastKnown?.updatedAt ? ` (${ctx.fmtTime(ctx.lastKnown.updatedAt)})` : "";
      return {
        title: "Couldn't check · relay unreachable",
        lastKnown: lk ? `last known: ${lk}${at}` : undefined,
        tone: "warn",
        retry: true,
      };
    }
    case "unknown":
      // Reasons `purged` (rev 5) and `legacy_redeemed` (rev 6). Never expired or picked up.
      return { title: "No longer on the relay · check the member list", tone: "muted" };
  }
}
