// GROW-S2 (Join 9) — the toast after the member presses Join on a citrate://invite link (paste box,
// pending banner). The legacy self-admit returns only once the group is joined; a rendezvous join
// returns the daemon's state, said in the contract §11 words (citrate-federation
// .agentile/planset/2026-10-19-betanet/JOIN_GROW_S2_RELAY.md). The full per-state rows are the
// Groups banner (M5); this is the one-line confirmation.
import type { JoinRedeemResult } from "../bridge/domains";

const INVITER = "the person who invited you";

export function redeemToast(r: JoinRedeemResult, groupListed: boolean): string {
  if (r.path === "legacy") return "You're in — welcome to the group.";
  const { state, reason, name } = r.join;
  const cluster = name || "the group";
  switch (state) {
    case "requesting":
      return "Asking to join…";
    case "waiting":
      return `Waiting for ${INVITER} to let you in`;
    case "joining":
      return `Joining ${cluster}…`;
    case "joined":
      return groupListed ? `Joined ${cluster}` : `Joining ${cluster}…`;
    case "taken":
      return groupListed ? `Joined ${cluster}` : "Someone else already used this invite";
    case "expired":
      return "This invite has expired";
    case "welcome_expired":
      return "This invite wasn't picked up in time. Ask for a new one.";
    case "revoked":
      return "This invite was withdrawn";
    case "dropped":
      return reason === "already_member" ? "Already a member" : "Your inviter's app couldn't add you";
    case "withdrawn":
      return "You cancelled this request";
    case "unknown":
      return "We couldn't find that invite";
    case "relay_unreachable":
      return "Couldn't check your invite · relay unreachable";
    case "relay_unsupported":
      return "Invites need an update on our side";
    case "reply_unverified":
      return "The reply to your invite couldn't be verified";
    default:
      return "Couldn't finish joining";
  }
}

/** Whether the invite stays offered after a Join. */
export function keepInviteAfter(_r: JoinRedeemResult): boolean {
  return false;
}
