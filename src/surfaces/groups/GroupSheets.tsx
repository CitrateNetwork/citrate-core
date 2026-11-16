// =====================================================================
// citrate-core — Groups › Chat sheets (0.5.4 FL design, board 02): Create a group, Join with an
// invite, Share a file. Each is a real dialog (shell/ModalDialog: focus starts on the safe choice,
// Tab stays inside, Escape closes, focus returns to the opener).
//
// Data sources (Rule 7):
//   create   comms-member-daemon `groups_create` (shell/slices/groups.createGroup)
//   join     invites.rs `invites_preview` / `invites_request` through HaveAnInvite's JoinCard: the
//            link is read first and nothing is sent until you press Join; the inviter approves
//   share    the cluster daemon (`cluster_share_file`) for a file on this node (`storage.list`) or
//            one picked from disk (added to this node first)
// =====================================================================
import { useEffect, useRef, useState, type ReactNode } from "react";
import { ModalDialog } from "../../shell/ModalDialog";
import type { Store } from "../../shell/store";
import type { Group } from "../../bridge/domains";
import { bridge } from "../../bridge";
import { JoinCard } from "../HaveAnInvite";
import { isInviteLink } from "../inviteLink";
import { createGroup, groupsSlice } from "../../shell/slices/groups";
import { addAndShareFile, clusterSlice, isJoined, joinCluster, loadMyFiles, selectClusterGroup, shareClusterFile } from "../../shell/slices/cluster";

function Sheet({ title, onClose, children, focusKey }: { title: string; onClose: () => void; children: ReactNode; focusKey: string }) {
  const id = "sheet-" + focusKey;
  return (
    <ModalDialog labelledBy={id} onEscape={onClose} focusKey={focusKey} register="charter" zIndex={60} panelStyle={{ width: "min(520px, 100%)" }}>
        <div className="grp-dialog">
          <div className="grp-head">
            <h2 id={id} className="grp-h2" style={{ fontSize: 17 }}>{title}</h2>
            <span className="grp-spacer" />
            <button className="btn btn-ghost btn-sm" onClick={onClose} aria-label={`Close: ${title}`}>Close</button>
          </div>
          {children}
        </div>
    </ModalDialog>
  );
}

// Friendly labels over the daemon's kinds (dm | channel | forum); kind is not cosmetic.
export const KINDS: { id: Group["kind"]; label: string; note: string }[] = [
  { id: "forum", label: "Working", note: "A working group with a fuller roster and roles." },
  { id: "channel", label: "Circle", note: "A standing space for a team or community." },
  { id: "dm", label: "Direct", note: "A private conversation between a few people." },
];

export function CreateGroupSheet({ store, onClose }: { store: Store; onClose: () => void }) {
  const [name, setName] = useState("");
  const [kind, setKind] = useState<Group["kind"]>("forum");
  const st = groupsSlice.use();
  const create = async () => {
    const n = name.trim();
    if (!n) return;
    await createGroup(kind, n);
    if (!groupsSlice.get().error) {
      onClose();
      store.go("groups/members");
    }
  };
  return (
    <Sheet title="Create a group" onClose={onClose} focusKey="create">
      <label className="grp-field" htmlFor="grp-name">
        Name
        <input id="grp-name" className="input" placeholder="Orchard Lab" value={name} maxLength={80} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void create()} />
      </label>
      <fieldset style={{ border: "none", padding: 0, margin: 0 }}>
        <legend className="grp-field">Kind</legend>
        <span className="grp-seg">
          {KINDS.map((k) => (
            <button key={k.id} type="button" className={"btn btn-sm" + (kind === k.id ? " btn-secondary" : " btn-ghost")} aria-pressed={kind === k.id} title={k.note} onClick={() => setKind(k.id)}>
              {k.label}
            </button>
          ))}
        </span>
      </fieldset>
      <p className="grp-note">You are the owner. Invite people next; each gets a link only they can redeem.</p>
      {st.error && <div role="alert" className="grp-alert">{st.error}</div>}
      <div className="grp-inline" style={{ justifyContent: "flex-end" }}>
        <button className="btn" data-autofocus onClick={onClose}>Cancel</button>
        <button className="btn btn-primary" disabled={!name.trim() || st.creating} onClick={() => void create()}>
          {st.creating ? "Creating" : "Create and invite"}
        </button>
      </div>
    </Sheet>
  );
}

/** The wording for a code that does not resolve. Never "expired" for an unknown code. */
export const UNKNOWN_INVITE = "We couldn't find that invite. Check the link with the person who sent it.";

export function JoinInviteSheet({ store, memberOf, onClose }: { store: Store; memberOf: string[]; onClose: () => void }) {
  const [value, setValue] = useState("");
  const [link, setLink] = useState<string | null>(null);
  const [bad, setBad] = useState(false);
  const open = () => {
    const v = value.trim();
    if (!v) return;
    if (!isInviteLink(v)) {
      setBad(true);
      setLink(null);
      return;
    }
    setBad(false);
    setLink(v);
  };
  return (
    <Sheet title="Join with an invite" onClose={onClose} focusKey="join">
      <label className="grp-field" htmlFor="grp-invite">
        Invite link
        <input id="grp-invite" className="input mono" placeholder="citrate://invite?…" value={value} onChange={(e) => { setValue(e.target.value); setBad(false); }} onKeyDown={(e) => e.key === "Enter" && open()} />
      </label>
      {bad && <div role="alert" className="grp-alert">{UNKNOWN_INVITE}</div>}
      {link ? (
        <JoinCard
          link={link}
          canJoin={store.canJoinNow()}
          memberOf={memberOf}
          onSent={() => {
            store.toast("Request sent. You're added once the person who invited you approves it.");
            onClose();
          }}
          onClose={() => setLink(null)}
        />
      ) : (
        <p className="grp-note">You see which group it is for before anything is sent. Joining sends a request; the person who invited you approves it.</p>
      )}
      <div className="grp-inline" style={{ justifyContent: "flex-end" }}>
        <button className="btn" data-autofocus onClick={onClose}>Cancel</button>
        {!link && <button className="btn btn-primary" disabled={!value.trim()} onClick={open}>Check invite</button>}
      </div>
    </Sheet>
  );
}

const humanBytes = (n: number) => (!n || n < 0 ? "—" : n >= 1e6 ? `${Math.round(n / 1e6)} MB` : `${Math.max(1, Math.round(n / 1e3))} KB`);

export function ShareFileSheet({ store, groupId, onClose }: { store: Store; groupId: string; onClose: () => void }) {
  const cl = clusterSlice.use();
  const joined = isJoined(cl, groupId);
  const [cid, setCid] = useState("");
  const [over, setOver] = useState(false);
  const loaded = useRef(false);
  useEffect(() => {
    if (loaded.current) return;
    loaded.current = true;
    if (clusterSlice.get().selectedId !== groupId) void selectClusterGroup(groupId);
    void loadMyFiles();
  }, []);
  // Desktop drag and drop: a dropped file is added to this node, then shared with the group.
  useEffect(() => {
    if (bridge.mode !== "tauri") return;
    let un: (() => void) | undefined;
    let cancelled = false;
    void (async () => {
      try {
        const { getCurrentWebview } = await import("@tauri-apps/api/webview");
        const u = await getCurrentWebview().onDragDropEvent((event) => {
          const t = event.payload.type;
          setOver(t === "over" || t === "enter");
          if (t === "drop") for (const path of event.payload.paths) void addAndShareFile(groupId, path);
        });
        if (cancelled) u();
        else un = u;
      } catch {
        /* drag and drop unavailable: the picker and the node's files still work */
      }
    })();
    return () => {
      cancelled = true;
      if (un) un();
    };
  }, [groupId]);
  const browse = async () => {
    if (bridge.mode !== "tauri") {
      store.toast("Picking a file from disk needs the desktop app. Pick one of your node's files below.");
      return;
    }
    const spec = ["@tauri-apps", "plugin-dialog"].join("/");
    try {
      const mod = (await import(/* @vite-ignore */ spec)) as { open?: (o: unknown) => Promise<string | string[] | null> };
      const picked = mod.open ? await mod.open({ multiple: true }) : null;
      const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
      for (const p of paths) void addAndShareFile(groupId, p);
    } catch {
      store.toast("Couldn't open the file picker. Pick one of your node's files below.");
    }
  };
  return (
    <Sheet title="Share a file with the group" onClose={onClose} focusKey="share">
      <p className="grp-note">The group's cluster co-pins it, so the file stays available while any member holding it is online.</p>
      {cl.error && <div role="alert" className="grp-alert">{cl.error}</div>}
      {!joined ? (
        <div className="fl-card grp-pad">
          <b>You're not in this group's cluster yet</b>
          <span className="grp-muted" style={{ fontSize: 12 }}>Sharing a file needs this machine in the cluster. Joining contributes storage and keeps you in sync.</span>
          <span>
            <button className="btn btn-primary btn-sm" disabled={cl.joining} onClick={() => void joinCluster(groupId)}>{cl.joining ? "Joining" : "Join the cluster"}</button>
          </span>
        </div>
      ) : (
        <>
          <div className="fl-card grp-pad" style={{ borderStyle: "dashed", borderColor: over ? "var(--accent)" : "var(--line-2)", alignItems: "flex-start" }}>
            <span style={{ fontSize: 13 }}>{over ? "Drop to share" : "Drag a file here, or pick one"}</span>
            <button className="btn btn-sm" onClick={() => void browse()}>Pick a file from disk</button>
          </div>
          <span className="eyebrow">Or one of your node's files</span>
          {cl.myFiles.length === 0 ? (
            <span className="grp-note">No files on your node yet. Add some in Node › Storage › Files.</span>
          ) : (
            <ul className="grp-list">
              {cl.myFiles.map((f) => {
                const already = cl.status?.sharedFiles.includes(f.cid);
                return (
                  <li key={f.cid} className="grp-list-row">
                    <span className="grp-stack" style={{ flex: 1 }}>
                      <span className="mono" style={{ fontSize: 11.5, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{f.cid}</span>
                      <span className="grp-faint">{humanBytes(f.sizeBytes)} · {f.pinState}</span>
                    </span>
                    {already ? <span className="fl-chip fl-chip-ok">shared</span> : <button className="btn btn-sm" disabled={cl.sharing === f.cid} onClick={() => void shareClusterFile(groupId, f.cid)}>Share</button>}
                  </li>
                );
              })}
            </ul>
          )}
          <details className="grp-details">
            <summary>Advanced: share by CID</summary>
            <span className="grp-inline">
              <label className="sr-only" htmlFor="grp-cid">CID</label>
              <input id="grp-cid" className="input mono" placeholder="bafy…" value={cid} onChange={(e) => setCid(e.target.value)} />
              <button className="btn btn-sm" disabled={!cid.trim()} onClick={() => { void shareClusterFile(groupId, cid.trim()); setCid(""); }}>Share CID</button>
            </span>
          </details>
        </>
      )}
      <div className="grp-inline" style={{ justifyContent: "flex-end" }}>
        <button className="btn" data-autofocus onClick={onClose}>Done</button>
      </div>
    </Sheet>
  );
}
