// GROW-S1 task 11 — the inviter's links on a group: a one-time direct invite (short link + QR) and a
// public link (referral code, display only). Every link here comes from Rust (`bridge.join`); none
// carries a group id or a wallet address. See inviteFlow.ts for the steps and data sources.
import { useState, type CSSProperties } from "react";
import { bridge } from "../bridge";
import type { JoinQr } from "../bridge/domains";
import { QrCode } from "../fleet/FleetWizard";
import type { Store } from "../shell/store";
import { JOIN_GOALS, type JoinGoalId } from "./referral";
import {
  createDirectInvite,
  createPublicLink,
  expiryLabel,
  retryDirectInvite,
  revokeDirectInvite,
  type DirectOutcome,
  type InviteDeps,
  type PublicOutcome,
} from "./inviteFlow";

const GOAL_LABEL: Record<JoinGoalId, string> = {
  general: "General",
  storage: "Storage",
  training: "Training",
  inference: "Inference",
  dapps: "Dapps",
};

export interface InviteLinksViewProps {
  direct: DirectOutcome | null;
  pub: PublicOutcome | null;
  busy: "direct" | "public" | null;
  goal: JoinGoalId;
  /** QR modules per share URL, filled in by Rust once a link exists. */
  qr: Record<string, JoinQr>;
  onGoal: (g: JoinGoalId) => void;
  onCreateDirect: () => void;
  onCreatePublic: () => void;
  onCopy: (text: string) => void;
  onRevokeDirect: () => void;
  onRevokePublic: () => void;
  onRetryAttach: () => void;
}

const linkBox: CSSProperties = {
  flex: 1,
  minWidth: 0,
  fontSize: 11,
  color: "var(--tx-2)",
  border: "1px solid var(--line-1)",
  borderRadius: "var(--r-1)",
  padding: "8px 10px",
  background: "var(--srf-1)",
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
};
const note: CSSProperties = { fontSize: 9.5, color: "var(--tx-3)", lineHeight: 1.5 };

function LinkRow({ url, testId, onCopy }: { url: string; testId: string; onCopy: (t: string) => void }) {
  return (
    <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
      <span className="mono" style={linkBox}>{url}</span>
      <button className="btn btn-secondary btn-sm" data-testid={testId} onClick={() => onCopy(url)}>Copy link</button>
    </div>
  );
}

export function InviteLinksView(p: InviteLinksViewProps) {
  const d = p.direct;
  const pub = p.pub;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 11.5, color: "var(--tx-2)" }}>
        What the group is for
        <select className="input" value={p.goal} onChange={(e) => p.onGoal(e.target.value as JoinGoalId)} style={{ width: "auto", padding: "4px 8px", fontSize: 11.5 }}>
          {JOIN_GOALS.map((g) => (
            <option key={g} value={g}>{GOAL_LABEL[g]}</option>
          ))}
        </select>
      </label>

      {/* Direct invite */}
      {!d || d.kind === "error" ? (
        <>
          <button className="btn btn-primary btn-sm" data-testid="invite-direct-create" onClick={p.onCreateDirect} disabled={p.busy !== null} style={{ alignSelf: "flex-start" }}>
            {p.busy === "direct" ? "Creating…" : "Create an invite link"}
          </button>
          <span className="mono" style={note}>one-time link · whoever opens it joins in one click · you can be offline</span>
          {d?.kind === "error" && <span role="alert" style={{ fontSize: 11.5, color: "var(--danger)", lineHeight: 1.5 }}>{d.message}</span>}
        </>
      ) : d.kind === "link" && d.minted.shareUrl ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span style={{ fontSize: 11.5, fontWeight: 500, color: "var(--tx-2)" }}>One-time invite link</span>
          <LinkRow url={d.minted.shareUrl} testId="invite-direct-copy" onCopy={p.onCopy} />
          <div style={{ display: "flex", gap: 14, alignItems: "flex-start", flexWrap: "wrap" }}>
            {p.qr[d.minted.shareUrl] && <QrCode qr={p.qr[d.minted.shareUrl]} px={3} testId="invite-direct-qr" label="Invite link QR code" />}
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>{expiryLabel(d.minted.exp)} · works once</span>
              <span className="mono" style={note}>Send it privately. Anyone who has the whole link can use it, so treat it like a key.</span>
              <button className="btn btn-ghost btn-sm" data-testid="invite-direct-revoke" onClick={p.onRevokeDirect} disabled={p.busy !== null} style={{ alignSelf: "flex-start", color: "var(--tx-3)" }}>Revoke</button>
            </div>
          </div>
        </div>
      ) : d.kind === "pending" ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <span style={{ fontSize: 11.5, color: "var(--tx-2)" }}>The invite was reserved but isn't ready to share yet{d.minted.attachError ? ` (${d.minted.attachError.replace(/^join:\s*/, "")})` : ""}.</span>
          <span style={{ display: "flex", gap: 8 }}>
            <button className="btn btn-secondary btn-sm" data-testid="invite-direct-retry" onClick={p.onRetryAttach} disabled={p.busy !== null}>Retry</button>
            <button className="btn btn-ghost btn-sm" data-testid="invite-direct-revoke" onClick={p.onRevokeDirect} disabled={p.busy !== null} style={{ color: "var(--tx-3)" }}>Cancel invite</button>
          </span>
        </div>
      ) : d.kind === "fallback" ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span style={{ fontSize: 11.5, color: "var(--warn)", lineHeight: 1.5 }}>
            Short invite links are not available right now, so this is an app-only link. It opens for people who already have Citrate installed; anyone else needs to install it first, then paste the link in Groups.
          </span>
          <LinkRow url={d.relayLink} testId="invite-direct-copy" onCopy={p.onCopy} />
          <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <span className="mono" style={note}>one-time · expires in 14 days</span>
            <button className="btn btn-ghost btn-sm" data-testid="invite-direct-revoke" onClick={p.onRevokeDirect} disabled={p.busy !== null} style={{ color: "var(--tx-3)" }}>Revoke</button>
          </span>
        </div>
      ) : null}

      <div style={{ height: 1, background: "var(--line-1)", margin: "2px 0" }} />

      {/* Public link */}
      <span style={{ fontSize: 11.5, fontWeight: 500, color: "var(--tx-2)" }}>Public link</span>
      <p style={{ fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.6, margin: 0 }}>
        Post it anywhere. It shows your name and this group's name, and points people to Citrate. It can't add anyone to the group by itself: send a one-time link for that.
      </p>
      {pub?.kind === "link" && pub.minted.shareUrl ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <LinkRow url={pub.minted.shareUrl} testId="invite-public-copy" onCopy={p.onCopy} />
          <div style={{ display: "flex", gap: 14, alignItems: "flex-start", flexWrap: "wrap" }}>
            {p.qr[pub.minted.shareUrl] && <QrCode qr={p.qr[pub.minted.shareUrl]} px={3} testId="invite-public-qr" label="Public link QR code" />}
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>{expiryLabel(pub.minted.exp)}</span>
              <button className="btn btn-ghost btn-sm" data-testid="invite-public-revoke" onClick={p.onRevokePublic} disabled={p.busy !== null} style={{ alignSelf: "flex-start", color: "var(--tx-3)" }}>Revoke</button>
            </div>
          </div>
        </div>
      ) : (
        <>
          <button className="btn btn-secondary btn-sm" data-testid="invite-public-create" onClick={p.onCreatePublic} disabled={p.busy !== null} style={{ alignSelf: "flex-start" }}>
            {p.busy === "public" ? "Creating…" : "Create a public link"}
          </button>
          {pub?.kind === "error" && <span role="alert" style={{ fontSize: 11.5, color: "var(--danger)", lineHeight: 1.5 }}>{pub.message}</span>}
        </>
      )}
    </div>
  );
}

const deps: InviteDeps = {
  get invites() {
    return bridge.invites;
  },
  get join() {
    return bridge.join;
  },
};

/** Copy without echoing the link into a toast (a direct link carries its one-time key). */
export function copyLink(store: Pick<Store, "toast">, text: string): void {
  const p = navigator.clipboard?.writeText(text);
  if (p) p.then(() => store.toast("Link copied.")).catch(() => store.toast("Couldn't copy. Select the link and copy it."));
  else store.toast("Couldn't copy. Select the link and copy it.");
}

export function InviteLinks({ store, groupId, groupName, onChanged }: { store: Store; groupId: string; groupName: string; onChanged?: () => void }) {
  const [direct, setDirect] = useState<DirectOutcome | null>(null);
  const [pub, setPub] = useState<PublicOutcome | null>(null);
  const [busy, setBusy] = useState<"direct" | "public" | null>(null);
  const [goal, setGoal] = useState<JoinGoalId>("general");
  const [qr, setQr] = useState<Record<string, JoinQr>>({});

  const loadQr = (url: string | null | undefined) => {
    if (!url) return;
    bridge.join
      .shareQr(url)
      .then((m) => setQr((q) => ({ ...q, [url]: m })))
      .catch(() => {
        /* no QR (web preview): the link and Copy still work */
      });
  };

  const run = async (which: "direct" | "public", f: () => Promise<void>) => {
    if (busy) return;
    setBusy(which);
    try {
      await f();
    } finally {
      setBusy(null);
    }
  };

  return (
    <InviteLinksView
      direct={direct}
      pub={pub}
      busy={busy}
      goal={goal}
      qr={qr}
      onGoal={setGoal}
      onCopy={(t) => copyLink(store, t)}
      onCreateDirect={() =>
        void run("direct", async () => {
          const out = await createDirectInvite(deps, groupId, groupName, goal);
          setDirect(out);
          if (out.kind === "link") loadQr(out.minted.shareUrl);
          onChanged?.();
        })
      }
      onRetryAttach={() =>
        void run("direct", async () => {
          if (direct?.kind !== "pending") return;
          const out = await retryDirectInvite(deps, direct.minted.code, direct.relayToken);
          setDirect(out.kind === "error" ? { ...direct } : out);
          if (out.kind === "error") store.toast(out.message);
          if (out.kind === "link") loadQr(out.minted.shareUrl);
        })
      }
      onRevokeDirect={() =>
        void run("direct", async () => {
          if (!direct || direct.kind === "error") return;
          try {
            await revokeDirectInvite(deps, groupId, direct.kind === "fallback" ? null : direct.minted.code, direct.relayToken);
            setDirect(null);
            store.toast("Invite revoked.");
            onChanged?.();
          } catch (e) {
            store.toast("Couldn't revoke that invite: " + (e instanceof Error ? e.message : String(e)).replace(/^join:\s*/, ""));
          }
        })
      }
      onCreatePublic={() =>
        void run("public", async () => {
          const out = await createPublicLink(deps, groupName, goal);
          setPub(out);
          if (out.kind === "link") loadQr(out.minted.shareUrl);
        })
      }
      onRevokePublic={() =>
        void run("public", async () => {
          if (pub?.kind !== "link") return;
          try {
            await bridge.join.revoke(pub.minted.code);
            setPub(null);
            store.toast("Public link revoked.");
          } catch (e) {
            store.toast("Couldn't revoke that link: " + (e instanceof Error ? e.message : String(e)).replace(/^join:\s*/, ""));
          }
        })
      }
    />
  );
}
