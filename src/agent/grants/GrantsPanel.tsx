// =====================================================================
// HUP-S2.1 — the Grants panel (Settings > App > Hermes folder access).
//
// Lists every grant with its status and, for full access, a live countdown.
// Grant a folder: pick it, then tick read and/or write (separate grants; read
// never implies write). Revoke any live grant. Read-only full access over the
// home folder is a 24 h window behind a HIC-1 confirmation: the first click only
// prepares it and shows exactly what is being allowed, with its own short
// countdown; nothing is granted until the member confirms that specific id.
// A corrupted grant file grants nothing; the panel says so and offers a reset.
// =====================================================================
import { useCallback, useEffect, useState } from "react";
import {
  DESKTOP_ONLY,
  errorMessage,
  fmtCountdown,
  syncMessage,
  type FullAccessConfirmation,
  type GrantsChange,
  type GrantsIo,
  type GrantsView,
} from "./grants";

export interface GrantsPanelProps {
  io: () => Promise<GrantsIo>;
  /** Unix seconds (injected for tests). */
  nowSecs?: () => number;
}

const realNow = () => Math.floor(Date.now() / 1000);

export function GrantsPanel({ io, nowSecs = realNow }: GrantsPanelProps) {
  const [x, setX] = useState<GrantsIo | null>(null);
  const [view, setView] = useState<GrantsView | null>(null);
  // When `view` was read, on this clock: countdowns run from the server's remaining seconds.
  const [readAt, setReadAt] = useState(0);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [read, setRead] = useState(true);
  const [write, setWrite] = useState(false);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<FullAccessConfirmation | null>(null);
  const [, setTick] = useState(0);

  const show = useCallback(
    (v: GrantsView) => {
      setView(v);
      setReadAt(nowSecs());
    },
    [nowSecs],
  );

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const got = await io();
        if (!live) return;
        setX(got);
        if (got.mode !== "tauri") return;
        const v = await got.invoke<GrantsView>("agent_grants_view", {});
        if (live) show(v);
      } catch (e) {
        if (live) setLoadError(errorMessage(e));
      }
    })();
    return () => {
      live = false;
    };
  }, [io, show]);

  // Re-render every second so the countdowns move.
  useEffect(() => {
    const t = setInterval(() => setTick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, []);

  if (x && x.mode !== "tauri") {
    return (
      <span style={{ fontSize: 12, color: "var(--tx-3)" }} data-testid="grants-desktop-only">
        {DESKTOP_ONLY}
      </span>
    );
  }
  if (loadError) {
    return (
      <span style={{ fontSize: 12, color: "var(--danger)" }}>
        Could not read the folder grants: {loadError}. Hermes is treated as having no folder access.
      </span>
    );
  }
  if (!x || !view) {
    return <span style={{ fontSize: 12, color: "var(--tx-3)" }}>Reading folder grants…</span>;
  }

  const elapsed = Math.max(0, nowSecs() - readAt);
  const left = (secs: number | null) => (secs === null ? null : Math.max(0, secs - elapsed));

  const run = async (f: () => Promise<GrantsChange>) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const c = await f();
      show(c.view);
      setNote(syncMessage(c.sync));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const grantFolder = async () => {
    if (!read && !write) {
      setError("Choose read, write, or both.");
      return;
    }
    let path: string | null;
    try {
      path = await x.pickFolder();
    } catch (e) {
      setError("Could not open the folder picker: " + errorMessage(e));
      return;
    }
    if (!path) return;
    const p = path;
    await run(() => x.invoke<GrantsChange>("agent_grants_add_folder", { path: p, read, write }));
  };

  const prepareFullAccess = async () => {
    setError(null);
    try {
      setConfirm(await x.invoke<FullAccessConfirmation>("agent_grants_full_access_prepare", {}));
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const confirmFullAccess = async (c: FullAccessConfirmation) => {
    await run(() => x.invoke<GrantsChange>("agent_grants_full_access_confirm", { id: c.id }));
    setConfirm(null);
  };

  const corrupted = view.status !== "ok";
  const fullRow = view.grants.find((g) => g.kind === "full_access" && g.status === "active") ?? null;
  const fullLeft = view.fullAccessRemainingSecs === null ? null : left(view.fullAccessRemainingSecs);
  const fullOn = fullRow !== null && fullLeft !== null && fullLeft > 0;
  const confirmLeft = confirm ? confirm.confirmBy - nowSecs() : 0;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }} data-testid="grants-panel">
      <span style={{ fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 }}>
        Hermes can only open files inside folders you grant here. Reading and writing are separate grants. Credential folders (.ssh, .aws, .gnupg, .kube), .env files outside a
        granted project, keychains and wallet data stay blocked under every grant.
      </span>

      {corrupted ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }} data-testid="grants-corrupted">
          <span style={{ fontSize: 12, color: "var(--danger)", lineHeight: 1.5 }}>{view.error ?? "The saved folder grants could not be read, so Hermes has no folder access."}</span>
          <span>
            <button className="btn btn-ghost btn-sm" data-testid="grants-reset" disabled={busy} onClick={() => void run(() => x.invoke<GrantsChange>("agent_grants_reset", {}))}>
              Set the unreadable file aside and start with no grants
            </button>
          </span>
        </div>
      ) : (
        <>
          {/* the list */}
          {view.grants.length === 0 ? (
            <span style={{ fontSize: 12, color: "var(--tx-3)" }}>No folders are granted. Hermes cannot open any of your files.</span>
          ) : (
            <div style={{ display: "flex", flexDirection: "column" }}>
              {view.grants.map((g) => {
                const liveRow = g.status === "active" || g.status === "not_yet_active";
                const rem = left(g.remainingSecs);
                return (
                  <div
                    key={g.id}
                    data-testid="grant-row"
                    style={{ display: "flex", gap: 10, alignItems: "center", borderBottom: "1px solid var(--line-1)", padding: "7px 0", opacity: liveRow ? 1 : 0.55 }}
                  >
                    <span className="mono" style={{ fontSize: 12, flex: 1, wordBreak: "break-all", color: "var(--tx-1)" }}>
                      {g.kind === "full_access" ? "Everything under " + g.root : g.root}
                    </span>
                    <span className="mono" style={{ fontSize: 10.5, color: g.access === "write" ? "var(--warn)" : "var(--tx-2)", textTransform: "uppercase" }}>
                      {g.access}
                    </span>
                    <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", minWidth: 120, textAlign: "right" }}>
                      {g.status === "active" && rem !== null ? `${fmtCountdown(rem)} left` : g.status === "active" ? "until revoked" : g.status.replace(/_/g, " ")}
                    </span>
                    {liveRow ? (
                      <button className="btn btn-ghost btn-sm" data-testid="revoke" disabled={busy} onClick={() => void run(() => x.invoke<GrantsChange>("agent_grants_revoke", { id: g.id }))}>
                        Revoke
                      </button>
                    ) : (
                      <span style={{ width: 62 }} />
                    )}
                  </div>
                );
              })}
            </div>
          )}

          {/* grant a folder */}
          <div style={{ display: "flex", gap: 14, alignItems: "center", flexWrap: "wrap" }}>
            <label style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12.5 }}>
              <input type="checkbox" data-testid="grant-read" checked={read} onChange={(e) => setRead(e.target.checked)} />
              Read
            </label>
            <label style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12.5 }}>
              <input type="checkbox" data-testid="grant-write" checked={write} onChange={(e) => setWrite(e.target.checked)} />
              Write (create and change files)
            </label>
            <button className="btn btn-ghost btn-sm" data-testid="grant-folder" disabled={busy} onClick={() => void grantFolder()}>
              Choose a folder to grant…
            </button>
          </div>

          {/* full access */}
          <div className="surface" style={{ padding: 12, display: "flex", flexDirection: "column", gap: 8, background: "var(--srf-1)" }}>
            <span className="eyebrow">Read-only full access · 24 h</span>
            {fullOn && fullRow ? (
              <span style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap" }}>
                <span style={{ fontSize: 12.5 }} data-testid="full-access-countdown">
                  On: Hermes may read files under {fullRow.root} for another <span className="mono">{fmtCountdown(fullLeft ?? 0)}</span>. It cannot write through this window.
                </span>
                <button className="btn btn-ghost btn-sm" data-testid="full-access-off" disabled={busy} onClick={() => void run(() => x.invoke<GrantsChange>("agent_grants_revoke", { id: fullRow.id }))}>
                  Turn off now
                </button>
              </span>
            ) : confirm ? (
              <div data-testid="full-access-confirm-card" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                <span className="mono" style={{ fontSize: 10, letterSpacing: ".08em", textTransform: "uppercase", color: "var(--warn)" }}>
                  HIC-1 · your explicit approval · read-only
                </span>
                <span style={{ fontSize: 12.5, lineHeight: 1.5, color: "var(--tx-1)" }}>{confirm.statement}</span>
                <span className="mono" style={{ fontSize: 11, color: confirmLeft > 0 ? "var(--tx-3)" : "var(--danger)" }}>
                  {confirmLeft > 0 ? `Confirm within ${fmtCountdown(confirmLeft)}` : "This confirmation timed out. Cancel and start again."}
                </span>
                <span style={{ display: "flex", gap: 8 }}>
                  <button className="btn btn-sm" data-testid="full-access-confirm" disabled={busy || confirmLeft <= 0} onClick={() => void confirmFullAccess(confirm)}>
                    Allow read-only access for 24 h
                  </button>
                  <button className="btn btn-ghost btn-sm" data-testid="full-access-cancel" onClick={() => setConfirm(null)}>
                    Cancel
                  </button>
                </span>
              </div>
            ) : (
              <span style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap" }}>
                <span style={{ fontSize: 12, color: "var(--tx-3)" }}>Off. Lets Hermes read (never write) anything under {view.fullAccessRoot} for 24 hours, then turns itself off.</span>
                <button className="btn btn-ghost btn-sm" data-testid="full-access-on" disabled={busy} onClick={() => void prepareFullAccess()}>
                  Turn on…
                </button>
              </span>
            )}
          </div>
        </>
      )}

      {error && (
        <span style={{ fontSize: 12, color: "var(--danger)" }} data-testid="grants-error">
          {error}
        </span>
      )}
      {note && (
        <span style={{ fontSize: 12, color: "var(--tx-2)" }} data-testid="grants-note">
          {note}
        </span>
      )}
    </div>
  );
}
