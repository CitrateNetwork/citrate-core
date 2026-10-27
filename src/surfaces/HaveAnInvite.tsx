// D-12 — the invitee's side of the one invite flow, the SAME component on Groups and in onboarding:
// the "Have an invite?" field, the "Join <group>?" screen for a link (pasted or opened from another
// app), and the requests this device sent (waiting, you're in, declined, revoked, expired).
// Nothing joins from the link alone: Join sends a request, and the person who invited you approves it.
// Data sources (Rule 7): bridge.invites.preview / request / myRequests / dismiss → src-tauri/src/
// invites.rs (the link itself, the relay's claims inbox, and the group daemon's Welcome).
import { useEffect, useRef, useState } from "react";
import { bridge } from "../bridge";
import type { InvitePreview, MyInviteRequest } from "../bridge/domains";
import type { AppState } from "../shell/state";
import type { Store } from "../shell/store";
import { reachedStep } from "../privacy/joinFunnel";
import { expiryText, isInviteLink } from "./inviteLink";

/** How often the requests this device sent are checked while this screen is open. */
export const REQUEST_CHECK_MS = 15_000;

const card = (tone: "accent" | "warn" | "ok" | "plain") => ({
  border: "1px solid " + (tone === "accent" ? "var(--accent)" : tone === "warn" ? "var(--warn)" : tone === "ok" ? "var(--ok)" : "var(--line-1)"),
  background: tone === "accent" ? "var(--accent-wash)" : tone === "warn" ? "var(--warn-bg)" : "var(--srf-1)",
  borderRadius: "var(--r-2)",
  padding: "12px 14px",
  display: "flex",
  flexDirection: "column" as const,
  gap: 8,
});
const row = { display: "flex", gap: 8, flexWrap: "wrap" as const, alignItems: "center" };
const small = { fontSize: 10.5, color: "var(--tx-3)", lineHeight: 1.5 };

function groupOf(name: string): string {
  return name || "the group";
}
function inviterOf(name: string): string {
  return name || "the person who invited you";
}
function cap(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}
function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e ?? "");
}

/** One request this device sent, as a card. */
export function RequestCard({ r, onDismiss }: { r: MyInviteRequest; onDismiss: () => void }) {
  const dismiss = (label: string) => (
    <button className="btn btn-ghost btn-sm" data-testid="invite-request-dismiss" onClick={onDismiss} style={{ color: "var(--tx-3)" }}>
      {label}
    </button>
  );
  const g = groupOf(r.groupName);
  const who = inviterOf(r.inviter);
  switch (r.state) {
    case "joined":
      return (
        <div style={card("ok")} data-testid="invite-request" data-state="joined">
          <span style={{ fontSize: 13 }}>You're in {g}.</span>
          <span style={row}>{dismiss("OK")}</span>
        </div>
      );
    case "declined":
      return (
        <div style={card("plain")} data-testid="invite-request" data-state="declined">
          <span style={{ fontSize: 13 }}>{cap(who)} declined your request to join {g}.</span>
          <span style={row}>{dismiss("Dismiss")}</span>
        </div>
      );
    case "revoked":
      return (
        <div style={card("plain")} data-testid="invite-request" data-state="revoked">
          <span style={{ fontSize: 13 }}>{cap(who)} cancelled this invite to {g}. Ask them for a new link if you still want to join.</span>
          <span style={row}>{dismiss("Dismiss")}</span>
        </div>
      );
    case "expired":
      return (
        <div style={card("plain")} data-testid="invite-request" data-state="expired">
          <span style={{ fontSize: 13 }}>Your invite to {g} expired before it was approved. Ask {who} for a new link.</span>
          <span style={row}>{dismiss("Dismiss")}</span>
        </div>
      );
    default:
      return (
        <div style={card("accent")} data-testid="invite-request" data-state="waiting">
          <span style={{ fontSize: 13 }}>Waiting for {who} to approve your request to join {g}…</span>
          <span className="mono" style={small}>
            You're added once they approve. If they decline or cancel the link, you'll see it here. · link {expiryText(r.expiresAt)}
          </span>
          {r.note && (
            <span className="mono" style={{ ...small, color: "var(--warn)" }} data-testid="invite-request-note">
              {r.note} We'll keep checking.
            </span>
          )}
          <span style={row}>{dismiss("Cancel request")}</span>
        </div>
      );
  }
}

type JoinPhase =
  | { kind: "checking" }
  | { kind: "error"; message: string }
  | { kind: "ready"; p: InvitePreview }
  | { kind: "sending"; p: InvitePreview }
  | { kind: "failed"; p: InvitePreview; message: string };

/** The "Join <group>?" screen for one link. */
export function JoinCard({
  link,
  canJoin,
  memberOf,
  onSent,
  onClose,
}: {
  link: string;
  canJoin: boolean;
  /** Group ids this member is already in. */
  memberOf: string[];
  onSent: () => void;
  onClose: () => void;
}) {
  const [phase, setPhase] = useState<JoinPhase>({ kind: "checking" });
  useEffect(() => {
    let live = true;
    setPhase({ kind: "checking" });
    bridge.invites
      .preview(link)
      .then((p) => live && setPhase({ kind: "ready", p }))
      .catch((e) => live && setPhase({ kind: "error", message: message(e) || "That invite link can't be used." }));
    return () => {
      live = false;
    };
  }, [link]);

  const close = (label: string) => (
    <button className="btn btn-ghost btn-sm" data-testid="invite-join-close" onClick={onClose} style={{ color: "var(--tx-3)" }}>
      {label}
    </button>
  );
  if (phase.kind === "checking") {
    return <div style={card("plain")} data-testid="invite-join" data-phase="checking"><span style={{ fontSize: 13 }}>Checking the invite…</span></div>;
  }
  if (phase.kind === "error") {
    return (
      <div style={card("warn")} data-testid="invite-join" data-phase="error">
        <span style={{ fontSize: 13, lineHeight: 1.5 }}>{phase.message}</span>
        <span style={row}>{close("Dismiss")}</span>
      </div>
    );
  }
  const p = phase.p;
  const g = groupOf(p.groupName);
  if (memberOf.includes(p.group)) {
    return (
      <div style={card("plain")} data-testid="invite-join" data-phase="member">
        <span style={{ fontSize: 13 }}>You're already in {g}.</span>
        <span style={row}>{close("OK")}</span>
      </div>
    );
  }
  if (p.expired) {
    return (
      <div style={card("warn")} data-testid="invite-join" data-phase="expired">
        <span style={{ fontSize: 13 }}>This invite to {g} has expired. Ask {inviterOf(p.inviter)} for a new link.</span>
        <span style={row}>{close("Dismiss")}</span>
      </div>
    );
  }
  const join = async () => {
    setPhase({ kind: "sending", p });
    try {
      await bridge.invites.request(link);
      onSent();
    } catch (e) {
      setPhase({ kind: "failed", p, message: message(e) });
    }
  };
  return (
    <div style={card("accent")} data-testid="invite-join" data-phase={phase.kind}>
      <span style={{ fontSize: 14, fontWeight: 500 }}>Join {g}?</span>
      <span style={{ fontSize: 12.5, color: "var(--tx-2)", lineHeight: 1.55 }}>
        {p.inviter ? `${p.inviter} invited you.` : "You were sent an invite."} Joining sends them a request; you're in once they approve it.
      </span>
      {phase.kind === "failed" && (
        <span role="alert" style={{ fontSize: 12, color: "var(--danger)", lineHeight: 1.5 }}>{phase.message}</span>
      )}
      {!canJoin && <span className="mono" style={small}>You can send the request once setup reaches the wallet step. Your invite stays here until then.</span>}
      <span style={row}>
        <button className="btn btn-primary btn-sm" data-testid="invite-join-send" disabled={!canJoin || phase.kind === "sending"} onClick={() => void join()}>
          {phase.kind === "sending" ? "Sending…" : phase.kind === "failed" ? "Try again" : "Join"}
        </button>
        {close("Cancel")}
        <span className="mono" style={small}>link {expiryText(p.expiresAt)}</span>
      </span>
    </div>
  );
}

/** The paste field: "Have an invite?" → paste the link → the Join screen. */
export function InvitePasteField({ onOpen }: { onOpen: (link: string) => void }) {
  const [open, setOpen] = useState(false);
  const [bad, setBad] = useState(false);
  const ref = useRef<HTMLInputElement>(null);
  const submit = () => {
    const v = ref.current?.value.trim() ?? "";
    if (!v) return;
    if (!isInviteLink(v)) {
      setBad(true);
      return;
    }
    setBad(false);
    if (ref.current) ref.current.value = "";
    setOpen(false);
    onOpen(v);
  };
  if (!open) {
    return (
      <button className="btn btn-ghost btn-sm" data-testid="invite-paste-open" onClick={() => setOpen(true)} style={{ alignSelf: "flex-start" }}>
        Have an invite?
      </button>
    );
  }
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }} data-testid="invite-paste">
      <span style={row}>
        <input ref={ref} className="input mono" data-testid="invite-paste-input" placeholder="Paste the invite link (citrate://invite?…)" style={{ flex: 1, minWidth: 220 }} onKeyDown={(e) => e.key === "Enter" && submit()} />
        <button className="btn btn-secondary btn-sm" data-testid="invite-paste-submit" onClick={submit}>Open</button>
        <button className="btn btn-ghost btn-sm" onClick={() => { setOpen(false); setBad(false); }} style={{ color: "var(--tx-3)" }}>Close</button>
      </span>
      {bad && <span role="alert" style={{ fontSize: 11.5, color: "var(--danger)" }}>That isn't an invite link. It starts with citrate://invite?</span>}
      <span className="mono" style={small}>you'll see which group it's for before anything is sent</span>
    </div>
  );
}

/**
 * The whole invitee side, shared by Groups and onboarding. `memberOf` is the group ids this member
 * is already in; `onJoined` runs when a request was approved and the join finished.
 */
export function HaveAnInvite({ store, s, memberOf = [], onJoined }: { store: Store; s: AppState; memberOf?: string[]; onJoined?: () => void }) {
  const [requests, setRequests] = useState<MyInviteRequest[]>([]);
  // Joined requests are gone from Rust once the join finished; keep their card until dismissed.
  const [joined, setJoined] = useState<MyInviteRequest[]>([]);
  const onJoinedRef = useRef(onJoined);
  onJoinedRef.current = onJoined;

  const check = async () => {
    try {
      const all = await bridge.invites.myRequests();
      const done = all.filter((r) => r.state === "joined");
      if (done.length) {
        reachedStep("join_group");
        setJoined((j) => [...j, ...done]);
        onJoinedRef.current?.();
      }
      setRequests(all.filter((r) => r.state !== "joined"));
    } catch {
      /* the saved requests can't be read right now: keep what is shown, try again next time */
    }
  };
  const waiting = requests.some((r) => r.state === "waiting");
  useEffect(() => {
    void check();
  }, []);
  useEffect(() => {
    if (!waiting) return;
    const t = setInterval(() => void check(), REQUEST_CHECK_MS);
    return () => clearInterval(t);
  }, [waiting]);

  const dismiss = async (r: MyInviteRequest) => {
    if (r.state === "joined") {
      setJoined((j) => j.filter((x) => x.id !== r.id));
      return;
    }
    try {
      await bridge.invites.dismiss(r.id);
      setRequests((all) => all.filter((x) => x.id !== r.id));
    } catch (e) {
      store.toast("Couldn't remove that request: " + message(e));
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }} data-testid="have-an-invite">
      {[...joined, ...requests].map((r) => (
        <RequestCard key={r.id + r.state} r={r} onDismiss={() => void dismiss(r)} />
      ))}
      {s.inviteLink ? (
        <JoinCard
          link={s.inviteLink}
          canJoin={store.canJoinNow()}
          memberOf={memberOf}
          onSent={() => {
            store.clearInviteLink();
            void check();
          }}
          onClose={() => store.clearInviteLink()}
        />
      ) : (
        <InvitePasteField onOpen={(link) => store.openInviteLink(link)} />
      )}
    </div>
  );
}
