// D-12 — the inviter's side of the one invite flow (a group owner or admin, in Groups › People):
// one [Invite someone] button that makes a link and copies it, and one Pending list with each open
// link (expiry, [Revoke]) and each incoming request (who, [Approve], [Decline]). Approve adds them
// through the normal AddMember / Welcome path. Nobody joins from the link alone.
// Data sources (Rule 7): bridge.invites.create / list / revoke / requests / approve / decline →
// src-tauri/src/invites.rs (this device's invites/pending.json and the relay's claims inbox).
import { useEffect, useState } from "react";
import { bridge } from "../bridge";
import type { HeldCheck, IncomingRequest, InviteView } from "../bridge/domains";
import type { Store } from "../shell/store";
import { expiryText, shortAddress } from "./inviteLink";

/** How often waiting requests are checked while the panel is open. */
export const REQUESTS_POLL_MS = 15_000;

/** The member-facing words for the outcome of a decline or revoke. */
export function declinedToast(notified: boolean): string {
  return notified
    ? "Declined. They'll see it in their app."
    : "Declined. Their app couldn't be told right now; their request shows as not approved once the link expires.";
}
export function revokedToast(notified: boolean): string {
  return notified
    ? "Invite revoked. It can't be used to join any more."
    : "Invite revoked. It can't be used to join any more. Anyone waiting on it couldn't be told right now; their request shows as not approved once the link expires.";
}

/** The member-facing words for a Check again on a held approval. */
export function checkedToast(r: HeldCheck): string {
  switch (r) {
    case "added":
      return "They're in the group. The invite is used up.";
    case "released":
      return "They aren't in the group, so the invite is open again. Approve their request again if you still want them in.";
    case "expired":
      return "They aren't in the group and the link has expired, so the invite is closed.";
    default:
      return "Nothing was waiting on this invite.";
  }
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e ?? "");
}

export function InvitePanel({
  store,
  groupId,
  inviterName,
  onAdded,
}: {
  store: Store;
  groupId: string;
  /** How the invitee's screen names you ("" for none). */
  inviterName: string;
  /** A request was approved (reload the roster). */
  onAdded?: () => void;
}) {
  const [invites, setInvites] = useState<InviteView[]>([]);
  const [listError, setListError] = useState<string | null>(null);
  const [requests, setRequests] = useState<IncomingRequest[]>([]);
  const [requestsError, setRequestsError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [label, setLabel] = useState("");

  const loadRequests = async () => {
    try {
      setRequests(await bridge.invites.requests(groupId));
      setRequestsError(null);
    } catch (e) {
      // Unknown is not "no requests": say the relay couldn't be asked.
      setRequestsError(message(e) || "Couldn't check for requests.");
    }
  };
  const refresh = async () => {
    let open: InviteView[] = [];
    try {
      open = await bridge.invites.list(groupId);
      setInvites(open);
      setListError(null);
    } catch (e) {
      setInvites([]);
      setListError(message(e) || "Couldn't read your invites.");
      return;
    }
    if (open.length) await loadRequests();
    else {
      setRequests([]);
      setRequestsError(null);
    }
  };
  useEffect(() => {
    void refresh();
  }, [groupId]);
  useEffect(() => {
    if (!invites.length) return;
    const t = setInterval(() => void loadRequests(), REQUESTS_POLL_MS);
    return () => clearInterval(t);
  }, [groupId, invites.length]);

  const copy = async (link: string) => {
    try {
      await navigator.clipboard?.writeText(link);
      return true;
    } catch {
      return false;
    }
  };
  const invite = async () => {
    setBusy("create");
    try {
      const made = await bridge.invites.create(groupId, label.trim(), inviterName);
      setLabel("");
      const copied = await copy(made.link);
      store.toast(
        (copied ? "Invite link copied. " : "Invite link made; copy it from Pending. ") +
          "Send it to the person you're inviting. When they open it, their request shows up here for you to approve.",
      );
      await refresh();
    } catch (e) {
      store.toast("Couldn't make an invite: " + message(e));
    } finally {
      setBusy(null);
    }
  };
  const revoke = async (inv: InviteView) => {
    setBusy(inv.id);
    try {
      store.toast(revokedToast((await bridge.invites.revoke(groupId, inv.id)).notified));
    } catch (e) {
      store.toast(message(e));
    } finally {
      setBusy(null);
      await refresh();
    }
  };
  const approve = async (r: IncomingRequest) => {
    setBusy(r.inviteId + r.address);
    try {
      await bridge.invites.approve(groupId, r.inviteId, r.address);
      store.toast(`Approved. ${shortAddress(r.address)} is added; their app finishes joining.`);
      onAdded?.();
    } catch (e) {
      store.toast(message(e));
    } finally {
      setBusy(null);
      await refresh();
    }
  };
  /** Check again on a held invite: the roster settles it (added, open again, or closed). */
  const check = async (inv: InviteView) => {
    setBusy(inv.id);
    try {
      const r = await bridge.invites.check(groupId, inv.id);
      store.toast(checkedToast(r));
      if (r === "added") onAdded?.();
    } catch (e) {
      store.toast(message(e));
    } finally {
      setBusy(null);
      await refresh();
    }
  };
  const decline = async (r: IncomingRequest) => {
    setBusy(r.inviteId + r.address);
    try {
      store.toast(declinedToast((await bridge.invites.decline(groupId, r.inviteId, r.address)).notified));
    } catch (e) {
      store.toast(message(e));
    } finally {
      setBusy(null);
      await refresh();
    }
  };

  const line = { display: "flex", alignItems: "center", gap: 10, padding: "8px 16px", borderTop: "1px solid var(--line-1)" };
  return (
    <div className="surface" style={{ display: "flex", flexDirection: "column" }} data-testid="invite-panel">
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "12px 16px" }}>
        <span style={{ flex: 1, minWidth: 0 }}>
          <span style={{ display: "block", fontSize: 13.5, fontWeight: 500 }}>Invite someone new</span>
          <span style={{ display: "block", fontSize: 12, color: "var(--tx-2)", lineHeight: 1.55, marginTop: 2 }}>
            You get a one-time link to send them. When they open it, they ask to join and you approve them here.
          </span>
        </span>
        <input className="input" data-testid="invite-label" value={label} maxLength={64} placeholder="Who's it for? (optional)" style={{ width: 170 }} onChange={(e) => setLabel(e.target.value)} />
        <button className="btn btn-primary btn-sm" data-testid="invite-someone" disabled={busy === "create"} onClick={() => void invite()}>
          {busy === "create" ? "Making…" : "Invite someone"}
        </button>
      </div>
      {listError && (
        <div role="alert" style={{ ...line, fontSize: 12, color: "var(--danger)", lineHeight: 1.5 }} data-testid="invite-list-error">
          {listError}
        </div>
      )}
      {(invites.length > 0 || requests.length > 0) && (
        <div style={{ padding: "10px 16px 4px", fontSize: 11, letterSpacing: ".04em", textTransform: "uppercase", color: "var(--accent-text)", borderTop: "1px solid var(--line-1)" }}>
          Pending
        </div>
      )}
      {requests.map((r) => {
        const key = r.inviteId + r.address;
        return (
          <div key={key} style={line} data-testid="invite-request-in">
            <span style={{ flex: 1, minWidth: 0 }}>
              <span style={{ fontSize: 12.5, fontWeight: 500 }}>A request to join from {shortAddress(r.address)}</span>
              <span className="mono" style={{ display: "block", fontSize: 9.5, color: "var(--tx-3)", wordBreak: "break-all" }}>{r.address}</span>
              <span style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", lineHeight: 1.5 }}>
                {r.approving
                  ? "being added · settle it with Check again on the invite"
                  : `asks to join through ${r.inviteLabel ? `“${r.inviteLabel}”` : "an invite link"} · confirm it's them before approving`}
              </span>
            </span>
            {!r.approving && (
              <>
                <button className="btn btn-primary btn-sm" data-testid="invite-approve" disabled={busy === key} onClick={() => void approve(r)}>Approve</button>
                <button className="btn btn-ghost btn-sm" data-testid="invite-decline" disabled={busy === key} onClick={() => void decline(r)} style={{ color: "var(--tx-3)" }}>Decline</button>
              </>
            )}
          </div>
        );
      })}
      {requestsError && (
        <div role="alert" style={{ ...line, fontSize: 11.5, color: "var(--warn)", lineHeight: 1.5 }} data-testid="invite-requests-error">
          {requestsError}
        </div>
      )}
      {invites.map((inv) => (
        <div key={inv.id} style={line} data-testid="invite-open">
          <span style={{ flex: 1, minWidth: 0 }}>
            <span style={{ fontSize: 12.5 }}>{inv.label ? `Invite link for ${inv.label}` : "Invite link"}</span>
            <span className="mono" style={{ display: "block", fontSize: 9.5, color: "var(--tx-3)" }}>
              {inv.approving
                ? `adding ${shortAddress(inv.approving)} · not confirmed yet`
                : `one use · ${expiryText(inv.expiresAt)}`}
            </span>
          </span>
          {inv.approving ? (
            <>
              <button className="btn btn-secondary btn-sm" data-testid="invite-check" disabled={busy === inv.id} onClick={() => void check(inv)}>Check again</button>
              <button className="btn btn-ghost btn-sm" data-testid="invite-cancel" disabled={busy === inv.id} onClick={() => void revoke(inv)} style={{ color: "var(--tx-3)" }}>Cancel</button>
            </>
          ) : (
            <>
              <button className="btn btn-ghost btn-sm" onClick={() => void copy(inv.link).then((ok) => store.toast(ok ? "Invite link copied." : "Couldn't copy the link."))}>Copy</button>
              <button className="btn btn-ghost btn-sm" data-testid="invite-revoke" disabled={busy === inv.id} onClick={() => void revoke(inv)} style={{ color: "var(--tx-3)" }}>Revoke</button>
            </>
          )}
        </div>
      ))}
    </div>
  );
}
