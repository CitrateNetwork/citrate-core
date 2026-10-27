// GROW-S1 task 12 — the waiting-invite card (onboarding + Groups) and the "Paste invite link or code"
// field. The invite itself is held in Rust; this card shows its status. Nothing joins silently: the
// member clicks Review (which resolves the invite) and then one explicit "Join <cluster>".
// Data sources (Rule 7): `bridge.join.pending*` (Rust keyring store + the citrate.ai join service).
import { useState } from "react";
import type { AppState, PendingJoinUi } from "../shell/state";
import type { Store } from "../shell/store";
import { v2InviteLinksEnabled } from "../join/v2Links";

export interface PendingJoinCardHandlers {
  onReview: () => void;
  onJoin: () => void;
  onCancel: () => void;
}

const card = (tone: "accent" | "warn" | "plain") => ({
  border: "1px solid " + (tone === "accent" ? "var(--accent)" : tone === "warn" ? "var(--warn)" : "var(--line-1)"),
  background: tone === "accent" ? "var(--accent-wash)" : tone === "warn" ? "var(--warn-bg)" : "var(--srf-1)",
  borderRadius: "var(--r-2)",
  padding: "14px 16px",
  display: "flex",
  flexDirection: "column" as const,
  gap: 8,
});

function expiryText(iso: string): string | null {
  const t = Date.parse(iso);
  return Number.isFinite(t) ? "expires " + new Date(t).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" }) : null;
}

export function PendingJoinCardView({ pj, canJoin, onReview, onJoin, onCancel }: { pj: PendingJoinUi; canJoin: boolean } & PendingJoinCardHandlers) {
  const cancel = (label = "Cancel") => (
    <button className="btn btn-ghost btn-sm" data-testid="pending-join-cancel" onClick={onCancel} style={{ color: "var(--tx-3)" }}>
      {label}
    </button>
  );
  const memoryNote = !pj.persisted ? (
    <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", lineHeight: 1.5 }}>
      This device couldn't save the invite, so it's kept only until the app quits. If you restart, paste the link again: an earlier invite may show in its place.
    </span>
  ) : null;
  const r = pj.resolved;

  if (pj.phase === "resolving") {
    return <div className="cc-fade-up" style={card("plain")} data-testid="pending-join"><span style={{ fontSize: 13 }}>Checking your invite…</span></div>;
  }
  if (pj.phase === "joining") {
    return <div className="cc-fade-up" style={card("accent")} data-testid="pending-join"><span style={{ fontSize: 13 }}>Joining {r?.clusterName || "the group"}…</span></div>;
  }
  if (pj.phase === "error") {
    return (
      <div className="cc-fade-up" style={card("warn")} data-testid="pending-join">
        <span style={{ fontSize: 13, lineHeight: 1.5 }}>{pj.error || "That invite couldn't be used."}</span>
        <span style={{ display: "flex", gap: 8 }}>
          {pj.retryable && (
            <button className="btn btn-secondary btn-sm" data-testid="pending-join-retry" onClick={onReview}>
              Try again
            </button>
          )}
          {cancel("Dismiss")}
        </span>
      </div>
    );
  }
  if (pj.phase === "ready" && r) {
    const who = r.inviter || "Someone";
    const verified = r.inviterVerified ? (
      <span className="mono" style={{ fontSize: 9.5, color: "var(--accent-text)", marginLeft: 6 }}>verified</span>
    ) : (
      <span className="mono" style={{ fontSize: 9.5, color: "var(--warn)", marginLeft: 6 }}>unverified</span>
    );
    const name = r.clusterName || "a group";
    const exp = expiryText(r.exp);
    if (r.kind === "referral") {
      return (
        <div className="cc-fade-up" style={card("plain")} data-testid="pending-join">
          <span style={{ fontSize: 13.5 }}><strong>{who}</strong>{verified} shared Citrate with you ({name}).</span>
          <span style={{ fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 }}>This is a public link, so it can't add you to a group. Ask {who} for a direct invite once you're set up.</span>
          <span style={{ display: "flex", gap: 8 }}>{cancel("Dismiss")}</span>
        </div>
      );
    }
    if (r.keyMissing || !r.actionable) {
      return (
        <div className="cc-fade-up" style={card("warn")} data-testid="pending-join">
          <span style={{ fontSize: 13.5 }}><strong>{who}</strong>{verified} invited you to <strong>{name}</strong>.</span>
          <span style={{ fontSize: 12, color: "var(--tx-1)", lineHeight: 1.5 }}>This link is missing its key (some apps cut off the part after #). Ask {who} to send the full link, then paste it here.</span>
          <span style={{ display: "flex", gap: 8 }}>{cancel("Dismiss")}</span>
        </div>
      );
    }
    return (
      <div className="cc-fade-up" style={card("accent")} data-testid="pending-join">
        <span style={{ fontSize: 13.5 }}>
          <strong>{who}</strong>{verified} invited you to <strong>{name}</strong>
          {r.goal ? <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", marginLeft: 8 }}>· {r.goal}</span> : null}
        </span>
        {exp && <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>one-time invite · {exp}</span>}
        {pj.error && <span role="alert" style={{ fontSize: 12, color: "var(--danger)", lineHeight: 1.5 }}>{pj.error}</span>}
        {canJoin ? (
          <span style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <button className="btn btn-primary btn-sm" data-testid="pending-join-join" onClick={onJoin}>Join {name}</button>
            {cancel()}
          </span>
        ) : (
          <>
            <span style={{ fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 }}>You can join {name} once setup finishes (sign in and your wallet).</span>
            <span style={{ display: "flex", gap: 8 }}>{cancel()}</span>
          </>
        )}
        {memoryNote}
      </div>
    );
  }
  // waiting
  return (
    <div className="cc-fade-up" style={card("plain")} data-testid="pending-join">
      {canJoin ? (
        <>
          <span style={{ fontSize: 13.5, fontWeight: 500 }}>You have an invite waiting</span>
          <span style={{ fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 }}>Review it to see who it's from and which group it's for. Nothing happens until you choose Join.</span>
          <span style={{ display: "flex", gap: 8 }}>
            <button className="btn btn-primary btn-sm" data-testid="pending-join-review" onClick={onReview}>Review invite</button>
            {cancel()}
          </span>
        </>
      ) : (
        <>
          <span style={{ fontSize: 13.5, fontWeight: 500 }}>You have an invite</span>
          <span style={{ fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 }}>You'll join the group you were invited to once setup finishes. You'll see who it's from and choose Join first.</span>
          <span style={{ display: "flex", gap: 8 }}>{cancel()}</span>
        </>
      )}
      {memoryNote}
    </div>
  );
}

/** The store-wired card. Renders nothing when no invite is waiting. */
export function PendingJoinCard({ store, s }: { store: Store; s: AppState }) {
  if (!s.pendingJoin) return null;
  return (
    <PendingJoinCardView
      pj={s.pendingJoin}
      canJoin={store.canJoinNow()}
      onReview={() => void store.reviewPendingJoin()}
      onJoin={() => void store.redeemPendingJoin()}
      onCancel={() => void store.cancelPendingJoin()}
    />
  );
}

/** "Paste invite link or code". `onSubmit` gets the raw text; the caller decides the path. */
export function InvitePasteField({ onSubmit, busy = false }: { onSubmit: (text: string) => void | Promise<void>; busy?: boolean }) {
  const [text, setText] = useState("");
  const submit = async () => {
    const t = text.trim();
    if (!t) return;
    await onSubmit(t);
    setText("");
  };
  return (
    <div style={{ display: "flex", flexWrap: "wrap", alignItems: "flex-end", gap: 8 }}>
      <label style={{ display: "flex", flexDirection: "column", gap: 4, flex: "1 1 260px", minWidth: 200 }}>
        <span className="lbl">{v2InviteLinksEnabled() ? "Paste invite link or code" : "Paste invite link"}</span>
        <input
          className="input mono"
          data-testid="invite-paste-input"
          placeholder={v2InviteLinksEnabled() ? "https://citrate.ai/join/… or a code" : "citrate://invite?…"}
          value={text}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void submit()}
        />
      </label>
      <button className="btn btn-secondary btn-sm" data-testid="invite-paste-submit" onClick={() => void submit()} disabled={busy || !text.trim()} style={{ flexShrink: 0 }}>
        Use invite
      </button>
    </div>
  );
}
