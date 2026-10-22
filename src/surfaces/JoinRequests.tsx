// GROW-S2 (Join 9, task M5) — the invitee's join requests (Groups banner, onboarding) and the
// inviter's per-invite state line. Copy comes from joinCopy.ts (contract §11); data from the
// rendezvous slice (member daemon views, Rule 7). Nothing here invents a state.
import { useEffect, useState } from "react";
import type { InviteViewDto, JoinViewDto } from "../bridge/domains";
import type { JoinActionError } from "../shell/slices/rendezvous";
import { inviteCopy, joinCopy, type StateCopy } from "./joinCopy";

export const fmtDate = (ms: number): string =>
  new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
export const fmtTime = (ms: number): string =>
  new Date(ms).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });

/** The current time, re-read every 10 s, so a view that goes stale while on screen turns into the
 *  unreachable copy without waiting for another event (§11 freshness). */
export function useNow(intervalMs = 10_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);
  return now;
}

const toneColor: Record<StateCopy["tone"], string> = {
  info: "var(--tx-1)",
  ok: "var(--ok, var(--tx-1))",
  warn: "var(--warn, var(--tx-1))",
  muted: "var(--tx-3)",
};

export interface JoinRequestsProps {
  joins: JoinViewDto[];
  lastKnown: Record<string, JoinViewDto>;
  /** Group ids in the daemon's group list (`listGroups`): "Joined" needs the group there. */
  listedGroups: string[];
  busy: string | null;
  /** When the app last got views from the daemon (the slice's `receivedAt`). */
  receivedAt: number;
  /** Override the clock (tests). */
  now?: number;
  onRetry: () => void;
  onCancel: (group: string) => void;
  onForget: (group: string) => void;
  /** The last failed cancel / forget (the slice's `error`). */
  error?: JoinActionError | null;
}

/** One row per join the daemon tracks. Renders nothing when there are none. */
export function JoinRequests({ joins, lastKnown, listedGroups, busy, receivedAt, now, onRetry, onCancel, onForget }: JoinRequestsProps) {
  const clock = useNow();
  const t = now ?? clock;
  if (joins.length === 0) return null;
  return (
    <div className="surface" data-testid="join-requests" style={{ display: "flex", flexDirection: "column" }}>
      {joins.map((j) => {
        const c = joinCopy(j, {
          cluster: j.name || "the group",
          groupListed: listedGroups.includes(j.group),
          lastKnown: lastKnown[j.group],
          fmtDate,
          fmtTime,
          now: t,
          receivedAt,
        });
        const isBusy = busy === j.group;
        return (
          <div key={j.group} data-state={j.state} style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 16px", borderTop: "1px solid var(--line-1)" }}>
            <span style={{ flex: 1, minWidth: 0 }}>
              <span style={{ fontSize: 13, fontWeight: 500, color: toneColor[c.tone] }}>{c.title}</span>
              {c.sub && <span style={{ display: "block", fontSize: 11.5, color: "var(--tx-3)" }}>{c.sub}</span>}
              {c.lastKnown && <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)" }}>{c.lastKnown}</span>}
            </span>
            {c.retry && <button className="btn btn-ghost btn-sm" disabled={isBusy} onClick={onRetry}>Retry</button>}
            {c.cancel && <button className="btn btn-ghost btn-sm" disabled={isBusy} onClick={() => onCancel(j.group)}>Cancel</button>}
            {c.forget && <button className="btn btn-ghost btn-sm" disabled={isBusy} style={{ color: "var(--tx-3)" }} onClick={() => onForget(j.group)}>Dismiss</button>}
          </div>
        );
      })}
    </div>
  );
}

export interface InviteStateLineProps {
  view: InviteViewDto | undefined;
  lastKnown: InviteViewDto | undefined;
  /** When the app last got views from the daemon (the slice's `receivedAt`). */
  receivedAt: number;
  /** Override the clock (tests). */
  now?: number;
  onRetry: () => void;
  onRemove: (joiner: string) => void;
}

/** The inviter's state for one invite, under its row in the invites list. Nothing until the
 *  daemon reports the rendezvous (an invite minted without one keeps its existing line). */
export function InviteStateLine({ view, lastKnown, receivedAt, now, onRetry, onRemove }: InviteStateLineProps) {
  const clock = useNow();
  if (!view) return null;
  const c = inviteCopy(view, { fmtDate, fmtTime, lastKnown, now: now ?? clock, receivedAt });
  return (
    <span data-testid="invite-state" data-state={view.state} style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 11.5, color: toneColor[c.tone] }}>
      <span>{c.title}</span>
      {c.lastKnown && <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>{c.lastKnown}</span>}
      {c.retry && <button className="btn btn-ghost btn-sm" onClick={onRetry}>Retry</button>}
      {c.remove && view.joiner && <button className="btn btn-ghost btn-sm" onClick={() => onRemove(view.joiner as string)}>Remove</button>}
    </span>
  );
}
