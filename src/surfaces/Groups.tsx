// =====================================================================
// citrate-core — Groups (CX-S3 redesign, Pass 1)
//
// The social spine, built 1:1 from design/CitrateCore.dc.html. A left rail (create / join / list)
// and a selected group with three tabs: Conversation, Roster, Cluster. RBAC is enforced at the
// RELAY, not the client (ADR-001) — the surface offers controls and shows the relay's honest error
// if you are not authorized; it never fakes success (Rule 1). Offboard stops at the Signature
// Ceremony. Role changes are not available until role grants are signed (#495, groupRoles.ts).
// The Cluster tab shares FILES (drag / Browse / pick from your files), never a hand-typed CID —
// normal people don't have a CID to paste.
// =====================================================================
import { useEffect, useRef, useState } from "react";
import { SurfaceProps } from "./shared";
import { bridge } from "../bridge";
import { Markdown } from "../components/Markdown";
import type { DirectorySearchHit, Group, GroupRole, ResolvedIdentity } from "../bridge/domains";
import { SOCIAL_BINDING_MSG_PREFIX } from "../bridge/domains";
import { visibleConversation } from "./groupControlMessages";
import { ingestDeviceLinkMessages, isDeviceLinkMessage } from "../fleet/deviceLinkShare";
import { addablePeople, filterPeople } from "./peopleDirectory";
import { HaveAnInvite } from "./HaveAnInvite";
import { InvitePanel } from "./InvitePanel";
import { RelayStatusChip } from "./RelayStatusChip";
import { flRoundCardsEnabled } from "../fl/roundCardsFlag";
import { GroupRoundsCard } from "../fl/GroupRoundsCard";
import { ROLE_CHANGES_AVAILABLE, ROLE_CHANGES_NOT_AVAILABLE } from "./groupRoles";
import {
  groupsSlice,
  refreshGroups,
  createGroup,
  selectGroup,
  sendMessage,
  reloadMessages,
  assignRole,
  offboardMember,
  addMemberToGroup,
  joinGroup,
  groupLabel,
} from "../shell/slices/groups";
import {
  clusterSlice,
  selectClusterGroup,
  loadMyFiles,
  joinCluster,
  leaveCluster,
  shareClusterFile,
  addAndShareFile,
  isJoined,
} from "../shell/slices/cluster";

type Tab = "chat" | "roster" | "cluster";

// Friendly labels over the real domain kinds (dm | channel | forum) — kind is not cosmetic.
const KINDS: { id: Group["kind"]; label: string; note: string }[] = [
  { id: "channel", label: "Circle", note: "A standing group space — the default for a team or community." },
  { id: "forum", label: "Working", note: "A topic-scoped working group with a fuller roster and roles." },
  { id: "dm", label: "Direct", note: "A private conversation between a few people." },
];

const ROLE_TONE: Record<GroupRole, { fg: string; bd: string; bg: string }> = {
  owner: { fg: "var(--accent-text)", bd: "var(--accent)", bg: "var(--accent-wash)" },
  admin: { fg: "var(--info)", bd: "var(--info)", bg: "var(--info-bg)" },
  member: { fg: "var(--tx-2)", bd: "var(--line-2)", bg: "transparent" },
  guest: { fg: "var(--tx-3)", bd: "var(--line-2)", bg: "transparent" },
  agent: { fg: "var(--info)", bd: "var(--info)", bg: "transparent" },
};

function shortAddr(a: string): string {
  if (!a) return "—";
  return a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a;
}
function initialsOf(s: string): string {
  const t = s.replace(/^0x/, "");
  return (t.slice(0, 2) || "··").toUpperCase();
}
function humanBytes(n: number): string {
  if (!n || n < 0) return "—";
  const mb = n / 1e6;
  if (mb >= 1) return `${Math.round(mb)} MB`;
  return `${Math.max(1, Math.round(n / 1e3))} KB`;
}

export function Groups({ store, s }: SurfaceProps) {
  const st = groupsSlice.use();
  const cl = clusterSlice.use();
  const [tab, setTab] = useState<Tab>("chat");
  const [createOpen, setCreateOpen] = useState(false);
  const [kind, setKind] = useState<Group["kind"]>("channel");
  const nameRef = useRef<HTMLInputElement>(null);
  const joinRef = useRef<HTMLInputElement>(null);
  const msgRef = useRef<HTMLInputElement>(null);
  const addrRef = useRef<HTMLInputElement>(null);
  const cidRef = useRef<HTMLInputElement>(null);
  const [cidAdvanced, setCidAdvanced] = useState(false);
  const [dropOver, setDropOver] = useState(false);
  const [pickerQ, setPickerQ] = useState(""); // CONNECT-S2 — add-member people-picker search
  // #61 find-via-X — search the opt-in directory for someone by their social handle.
  const [dirPlatform, setDirPlatform] = useState<"x" | "discord">("x");
  const [dirQ, setDirQ] = useState("");
  const [dirHits, setDirHits] = useState<DirectorySearchHit[]>([]);
  const [dirSearching, setDirSearching] = useState(false);

  const selected = st.groups.find((g) => g.id === st.selectedId) ?? null;
  const myWallet = typeof store.identity === "function" ? store.identity().wallet : "";
  // "Me" is the COMMS address (the roster key), NOT the wallet. Using the wallet here demoted owners
  // on every reload, because the roster never has a wallet-keyed seat (issue #55). Fall back to the
  // wallet only until the comms address is known (a brief window before refreshPeople lands).
  const myAddr = (s.commsAddr || myWallet || s.walletAddr || "").toLowerCase();
  // The DURABLE role comes from the comms-keyed navigator (s.myGroups, built in refreshPeople by
  // matching the comms self-address against every group's roster). It survives a reload with no
  // dependence on session-only names. Fall back to the roster/owner match only while the navigator
  // is still loading — never to the old wallet+iCreated heuristic, which was the bug.
  const myGroupRow = selected ? s.myGroups.find((g) => g.id === selected.id) : undefined;
  const iCreated = !!(selected && st.names[selected.id]);
  const myRole =
    myGroupRow?.myRole ??
    st.roster.find((r) => r.address.toLowerCase() === myAddr)?.role ??
    (selected && selected.owner.toLowerCase() === myAddr ? "owner" : "member");
  // RBAC is still enforced at the relay — this only decides which controls the UI offers.
  const canManage = myGroupRow ? myGroupRow.iManage : myRole === "owner" || myRole === "admin" || iCreated;

  // Verified, group-visible faces for the addresses on screen (ADR resolver). Self today;
  // cross-member when bindings are shared server-blind to groups. Fallback is the address avatar.
  const [faces, setFaces] = useState<Record<string, ResolvedIdentity>>({});
  const faceOf = (addr: string): ResolvedIdentity | undefined => faces[(addr || "").toLowerCase()];

  useEffect(() => {
    void refreshGroups();
    // Populate s.myGroups + s.commsAddr (the durable, comms-keyed role source) on mount so
    // canManage is correct immediately on load/reload — not just after visiting People (#55).
    void store.refreshPeople();
  }, []);

  useEffect(() => {
    const addrs = Array.from(new Set([...st.roster.map((r) => r.address), ...cl.peers.map((p) => p.address)])).filter(Boolean);
    if (addrs.length === 0) {
      setFaces({});
      return;
    }
    let cancelled = false;
    void bridge.social
      .resolve(addrs)
      .then((res) => {
        if (cancelled) return;
        const m: Record<string, ResolvedIdentity> = {};
        for (const r of res) m[r.address.toLowerCase()] = r;
        setFaces(m);
      })
      .catch(() => {
        /* honest: no faces resolved — the address avatar renders */
      });
    return () => {
      cancelled = true;
    };
  }, [st.roster, cl.peers]);

  // D1 — ingest binding-share control messages from the relay: recover-verify each peer's binding
  // (Rust) and, if any land, re-resolve so their faces appear. Control messages are hidden from view.
  useEffect(() => {
    const ctrl = st.messages.filter((m) => m.body.startsWith(SOCIAL_BINDING_MSG_PREFIX));
    if (ctrl.length === 0) return;
    let cancelled = false;
    void (async () => {
      let accepted = false;
      for (const m of ctrl) {
        try {
          const payload = JSON.parse(m.body.slice(SOCIAL_BINDING_MSG_PREFIX.length));
          if (await bridge.social.ingestBinding(m.sender, payload)) accepted = true;
        } catch {
          /* ignore a malformed control message */
        }
      }
      if (accepted && !cancelled) {
        const addrs = Array.from(new Set([...st.roster.map((r) => r.address), ...cl.peers.map((p) => p.address)])).filter(Boolean);
        const res = await bridge.social.resolve(addrs).catch(() => []);
        if (!cancelled) {
          const m: Record<string, ResolvedIdentity> = {};
          for (const r of res) m[r.address.toLowerCase()] = r;
          setFaces(m);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [st.messages]);

  // HUP-S8.1 follow-on: other members' DeviceLink shares. Core verifies every signature and that the
  // relay attributes the share to the member it speaks for; only then are the links kept.
  useEffect(() => {
    const shares = st.messages.filter((m) => isDeviceLinkMessage(m.body));
    if (shares.length === 0) return;
    void ingestDeviceLinkMessages(shares, (sender, body) => bridge.cluster.deviceLinksIngest(sender, body));
  }, [st.messages]);

  // Control messages never render in the conversation.
  const visibleMessages = visibleConversation(st.messages);

  // While the chat is open on a group, drain the mailbox on an interval so messages from other members
  // arrive live. `reloadMessages` folds drained messages into retained history (dedup + persist); an
  // unreachable relay/daemon is an honest no-op, never a fabricated message.
  useEffect(() => {
    if (tab !== "chat" || !selected) return;
    const id = selected.id;
    const timer = setInterval(() => void reloadMessages(id), 4000);
    return () => clearInterval(timer);
  }, [tab, selected?.id]);

  // When the Cluster tab opens for a group, drive the cluster slice to that group + load files.
  useEffect(() => {
    if (tab === "cluster" && selected) {
      void selectClusterGroup(selected.id);
      void loadMyFiles();
    }
  }, [tab, selected?.id]);

  // Tauri drag-drop → add + share to the current group's cluster (no CID typing).
  useEffect(() => {
    if (bridge.mode !== "tauri" || tab !== "cluster" || !selected) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void (async () => {
      try {
        const { getCurrentWebview } = await import("@tauri-apps/api/webview");
        const un = await getCurrentWebview().onDragDropEvent((event) => {
          const t = event.payload.type;
          setDropOver(t === "over" || t === "enter");
          if (t === "drop" && selected) for (const path of event.payload.paths) void addAndShareFile(selected.id, path);
        });
        if (cancelled) un();
        else unlisten = un;
      } catch {
        /* drag-drop unavailable — pick-from-your-files still works */
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, [tab, selected?.id]);

  const doCreate = () => {
    const n = nameRef.current?.value.trim() ?? "";
    if (!n) {
      store.toast("Name the group first.");
      return;
    }
    if (nameRef.current) nameRef.current.value = "";
    setCreateOpen(false);
    void createGroup(kind, n);
  };
  const doJoin = () => {
    const id = joinRef.current?.value.trim() ?? "";
    if (!id) return;
    if (joinRef.current) joinRef.current.value = "";
    void joinGroup(id);
  };
  const doSend = () => {
    const b = msgRef.current?.value.trim() ?? "";
    if (!b) return;
    if (msgRef.current) msgRef.current.value = "";
    void sendMessage(b);
  };
  const doAdd = () => {
    const v = addrRef.current?.value.trim() ?? "";
    if (!v) return;
    if (v.startsWith("@")) {
      store.toast("To invite someone by their @handle, press Invite someone below and send them the link. Citrate never looks up an address from a handle.");
      return;
    }
    if (!selected) return;
    if (addrRef.current) addrRef.current.value = "";
    void addMemberToGroup(selected.id, v);
  };

  // #61 find-via-X — resolve a social handle to its published address via the opt-in directory. Only
  // returns people who explicitly opted in (self-published, revocable); honest-empty otherwise (D-7).
  const doDirectorySearch = async () => {
    const q = dirQ.trim().replace(/^@/, "");
    if (!q) { setDirHits([]); return; }
    setDirSearching(true);
    try {
      setDirHits(await bridge.social.directorySearch(dirPlatform, q));
    } catch (e) {
      setDirHits([]);
      store.toast("Couldn't search the directory — " + (e instanceof Error ? e.message : String(e)));
    } finally {
      setDirSearching(false);
    }
  };

  // CONNECT-S2 — keep the add-member picker's people fresh while the People tab is open.
  useEffect(() => {
    if (tab === "roster" && selected && canManage) void store.refreshPeople();
  }, [tab, selected?.id, canManage]);

  /** The name shown for a group in an approval, so the approval says which group it acts on. */
  const groupNameFor = (groupId: string): string => {
    const g = st.groups.find((x) => x.id === groupId);
    return g ? groupLabel(st, g) : groupId;
  };
  /** #389: an approval acts on the group it was proposed for. If the selected group changed while the
   *  approval was open, refuse and send nothing, so an action is never applied to a group the member
   *  is no longer looking at. */
  const stillOnGroup = (groupId: string): boolean => {
    if (groupsSlice.get().selectedId === groupId) return true;
    store.toast(`The selected group changed after this was proposed, so nothing was sent to ${groupNameFor(groupId)}. Open that group and try again.`);
    return false;
  };
  const doRole = (groupId: string, address: string, role: GroupRole) => {
    const next: GroupRole = role === "admin" ? "member" : "admin";
    void store.requestSig({
      origin: "user wallet action",
      requester: "you · group admin",
      title: next === "admin" ? "Make this member an admin" : "Return this admin to member",
      rows: [
        { k: "Group", v: groupNameFor(groupId) },
        { k: "Member", v: shortAddr(address) },
        { k: "New role", v: next },
        { k: "Effect", v: "the member's role in this group changes" },
      ],
      cost: "—",
      sponsor: "you approve · one role change",
      sponsorColor: "var(--ok)",
      chainless: true,
      apply: () => {
        if (stillOnGroup(groupId)) void assignRole(groupId, address, next);
      },
    });
  };
  const doOffboard = (groupId: string, address: string) => {
    void store.requestSig({
      origin: "user wallet action",
      requester: "you · group admin",
      title: "Offboard this member",
      rows: [
        { k: "Group", v: groupNameFor(groupId) },
        { k: "Member", v: shortAddr(address) },
        { k: "Effect", v: "removed from messaging, roster, cluster, and roles" },
      ],
      cost: "—",
      sponsor: "you approve · one action",
      sponsorColor: "var(--danger)",
      chainless: true,
      warning: "This removes them across all planes in one epoch and rotates the group secret — they lose access everywhere at once. This cannot be undone.",
      apply: () => {
        if (!stillOnGroup(groupId)) return;
        void offboardMember(groupId, address);
        store.toast("Offboarding — the group secret rotates.");
      },
    });
  };

  const browseAndShare = async () => {
    if (!selected) return;
    if (bridge.mode !== "tauri") {
      store.toast("Adding files needs the desktop app — in the web preview, pick from your files below.");
      return;
    }
    // The native file dialog is an optional tauri plugin. Resolve it at runtime via a
    // computed specifier so the build never hard-depends on it; if it isn't present, guide
    // the user to drag-drop / pick-from-files instead (both work without it).
    const spec = ["@tauri-apps", "plugin-dialog"].join("/");
    let open: ((o: unknown) => Promise<string | string[] | null>) | null = null;
    try {
      const mod = (await import(/* @vite-ignore */ spec)) as { open?: (o: unknown) => Promise<string | string[] | null> };
      open = mod.open ?? null;
    } catch {
      open = null;
    }
    if (!open) {
      store.toast("Drag a file onto the box to add it, or pick one from your files below.");
      return;
    }
    try {
      const picked = await open({ multiple: true });
      const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
      for (const p of paths) void addAndShareFile(selected.id, p);
    } catch {
      store.toast("Couldn't open the file picker — drag a file onto the box, or pick one from your files below.");
    }
  };

  const joined = isJoined(cl, selected?.id ?? null);
  const clErr = cl.error;

  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: "100%", boxSizing: "border-box" }}>
      {/* ---------- top bar: group switcher (#59 — replaces the old 248px left rail so the
           surface is one pane, not a sidebar next to a sidebar) ---------- */}
      <div style={{ borderBottom: "1px solid var(--line-1)", background: "var(--srf-1)", padding: "12px 20px", display: "flex", flexDirection: "column", gap: 10, flexShrink: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 19, flexShrink: 0 }}>Groups</span>
          <RelayStatusChip />
          {/* horizontal group switcher — scrolls when there are many */}
          {st.groups.length > 0 && (
            <div style={{ display: "flex", gap: 6, overflowX: "auto", flex: 1, minWidth: 0, padding: "2px 0" }}>
              {st.groups.map((g) => {
                const on = g.id === st.selectedId;
                return (
                  <button
                    key={g.id}
                    onClick={() => { setTab("chat"); void selectGroup(g.id); }}
                    title={groupLabel(st, g)}
                    style={{ display: "inline-flex", alignItems: "center", gap: 7, padding: "6px 12px", borderRadius: 999, border: "1px solid " + (on ? "var(--accent)" : "var(--line-1)"), background: on ? "var(--srf-2)" : "transparent", color: "var(--tx-1)", cursor: "pointer", whiteSpace: "nowrap", flexShrink: 0, fontSize: 12.5, fontWeight: on ? 500 : 400 }}
                  >
                    <span className="mono" style={{ width: 20, height: 20, borderRadius: 6, border: "1px solid var(--line-2)", background: "#fff", color: "var(--tx-2)", display: "inline-flex", alignItems: "center", justifyContent: "center", fontSize: 9, fontWeight: 600 }}>{initialsOf(groupLabel(st, g))}</span>
                    <span style={{ maxWidth: 180, overflow: "hidden", textOverflow: "ellipsis" }}>{groupLabel(st, g)}</span>
                    {g.members.length ? <span className="mono" style={{ fontSize: 9, color: "var(--tx-3)" }}>{g.members.length}</span> : null}
                  </button>
                );
              })}
            </div>
          )}
          <span style={{ marginLeft: "auto", display: "flex", gap: 6, flexShrink: 0 }}>
            <button className="btn btn-secondary btn-sm" onClick={() => setCreateOpen((v) => !v)}>{createOpen ? "Close" : "New group"}</button>
          </span>
        </div>

        {st.groups.length === 0 && (
          <p style={{ fontSize: 12, lineHeight: 1.6, color: "var(--tx-3)", margin: 0 }}>
            No groups yet. Create one to bring people together, or join one you've been pointed to.
          </p>
        )}

        {createOpen && (
          <div style={{ display: "flex", flexWrap: "wrap", alignItems: "flex-end", gap: 10, border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: 12, background: "#fff" }}>
            <label style={{ display: "flex", flexDirection: "column", gap: 4, flex: "1 1 200px", minWidth: 160 }}>
              <span className="lbl">New group</span>
              <input ref={nameRef} className="input" placeholder="Name" onKeyDown={(e) => e.key === "Enter" && doCreate()} />
            </label>
            <div style={{ display: "flex", gap: 6, flexShrink: 0 }}>
              {KINDS.map((k) => (
                <button key={k.id} className={"btn btn-sm " + (kind === k.id ? "btn-secondary" : "btn-ghost")} onClick={() => setKind(k.id)} title={k.note}>
                  {k.label}
                </button>
              ))}
            </div>
            <button className="btn btn-primary btn-sm" onClick={doCreate} disabled={st.creating} style={{ flexShrink: 0 }}>{st.creating ? "Creating…" : "Create"}</button>
            <div style={{ display: "flex", alignItems: "flex-end", gap: 6, flex: "1 1 200px", minWidth: 180, borderLeft: "1px solid var(--line-1)", paddingLeft: 10 }}>
              <label style={{ display: "flex", flexDirection: "column", gap: 4, flex: 1, minWidth: 0 }}>
                {/* Not an invite: a member someone added by address (Add people / Add by address)
                    joins that group here from its Welcome (groups_join). */}
                <span className="lbl">Added by address? Join by group id</span>
                <input ref={joinRef} className="input" placeholder="grp_…" onKeyDown={(e) => e.key === "Enter" && doJoin()} />
              </label>
              <button className="btn btn-secondary btn-sm" onClick={doJoin} style={{ flexShrink: 0 }}>Join</button>
            </div>
          </div>
        )}

        {/* D-12 — the invitee's side (the same component as onboarding): "Have an invite?", the
            "Join <group>?" screen for a link, and the requests this device sent. */}
        <HaveAnInvite store={store} s={s} memberOf={st.groups.map((g) => g.id)} />

        {st.error && (
          <div role="alert" style={{ fontSize: 11.5, color: "var(--danger)", lineHeight: 1.5 }}>{st.error}</div>
        )}
      </div>

      {/* ---------- main (full width now the rail is gone) ---------- */}
      {!selected ? (
        <div className="lattice-dots" style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 14, padding: 48, textAlign: "center" }}>
          <div style={{ fontFamily: "var(--font-display)", fontWeight: 400, fontSize: 26 }}>Your groups live here.</div>
          <p style={{ fontSize: 13.5, lineHeight: 1.6, color: "var(--tx-2)", margin: 0, maxWidth: 460 }}>
            A group is a space with real roles — enforced at the relay, which only ever sees ciphertext. Reach stays in your groups. Talk, hold files together as a cluster, and grow it toward the network's milestones.
          </p>
        </div>
      ) : (
        <div style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "16px 24px 0", flexWrap: "wrap" }}>
            <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 22, minWidth: 0, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", maxWidth: "100%" }}>{groupLabel(st, selected)}</span>
            <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", padding: "2px 9px", borderRadius: 999, border: "1px solid var(--line-2)", color: "var(--tx-2)", flexShrink: 0 }}>
              {KINDS.find((k) => k.id === selected.kind)?.label ?? selected.kind}
            </span>
            <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", flexShrink: 0 }}>{selected.id.slice(0, 12)}…</span>
            <span style={{ display: "flex", gap: 2, background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: 2, marginLeft: "auto", flexShrink: 0 }}>
              {([["chat", "Conversation"], ["roster", "People"], ["cluster", "Cluster"]] as const).map(([id, label]) => {
                const on = tab === id;
                return (
                  <button key={id} onClick={() => setTab(id)} style={{ fontFamily: "var(--font-sans)", fontSize: 12, fontWeight: on ? 500 : 400, padding: "5px 12px", border: "none", borderRadius: 5, cursor: "pointer", background: on ? "var(--srf-2)" : "transparent", color: on ? "var(--tx-1)" : "var(--tx-2)" }}>
                    {label}
                  </button>
                );
              })}
            </span>
          </div>

          {/* ---- Conversation ---- */}
          {tab === "chat" && (
            <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", padding: "14px 24px 20px", gap: 12 }}>
              {flRoundCardsEnabled() && <GroupRoundsCard variant="chat" />}
              <div className="surface" style={{ flex: 1, minHeight: 280, overflow: "auto", padding: 16, display: "flex", flexDirection: "column", gap: 14 }}>
                {visibleMessages.length === 0 ? (
                  <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-3)", margin: "auto", textAlign: "center", maxWidth: 340 }}>
                    Nothing here yet. Say hello — messages are relayed to every member and readable by the roles you see in the roster.
                  </p>
                ) : (
                  visibleMessages.map((m) => {
                    const rosterRole = st.roster.find((r) => r.address.toLowerCase() === m.sender.toLowerCase())?.role;
                    const isAgent = rosterRole === "agent";
                    const you = m.sender.toLowerCase() === myAddr;
                    return (
                      <div key={m.id} style={{ display: "flex", gap: 10 }}>
                        <span className="mono" style={{ width: 26, height: 26, borderRadius: 999, border: isAgent ? "1px dashed var(--info)" : "1px solid var(--line-2)", background: you ? "var(--accent-wash)" : "#fff", color: "var(--tx-2)", display: "inline-flex", alignItems: "center", justifyContent: "center", fontSize: 9.5, fontWeight: 600, flexShrink: 0 }}>
                          {initialsOf(m.sender)}
                        </span>
                        <span style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 3 }}>
                          <span style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
                            <span style={{ fontSize: 12, fontWeight: 500 }}>{you ? "You" : faceOf(m.sender) ? "@" + faceOf(m.sender)!.handle : shortAddr(m.sender)}</span>
                            {isAgent && <span className="mono" style={{ fontSize: 8.5, letterSpacing: ".1em", textTransform: "uppercase", padding: "1px 6px", borderRadius: 999, border: "1px dashed var(--info)", color: "var(--info)" }}>agent</span>}
                            <span className="mono" style={{ fontSize: 9.5, color: "var(--tx-3)" }}>{m.ts ? new Date(m.ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : ""}</span>
                          </span>
                          <span style={{ fontSize: 13, lineHeight: 1.55, color: "var(--tx-1)", wordBreak: "break-word" }}><Markdown text={m.body} /></span>
                        </span>
                      </div>
                    );
                  })
                )}
              </div>
              <div style={{ display: "flex", gap: 10 }}>
                <input ref={msgRef} className="input" placeholder="Message the group…" onKeyDown={(e) => e.key === "Enter" && doSend()} style={{ flex: 1 }} />
                <button className="btn btn-primary" onClick={doSend} disabled={st.sending}>{st.sending ? "…" : "Send"}</button>
              </div>
            </div>
          )}

          {/* ---- People ---- */}
          {tab === "roster" && (
            <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "14px 24px 20px", display: "flex", flexDirection: "column", gap: 14 }}>
              {/* #59 — Add people, front and center: the primary connect action. Search the people you
                  already know (from any of your groups) → one-click add; or invite someone new. */}
              {canManage && (() => {
                const candidates = filterPeople(addablePeople(s.people, st.roster.map((r) => r.address)), pickerQ);
                return (
                  <div className="surface" style={{ display: "flex", flexDirection: "column", gap: 10, padding: "14px 16px" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <span style={{ fontSize: 13.5, fontWeight: 500 }}>Add people</span>
                      <span className="mono" style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".08em", textTransform: "uppercase", color: "var(--tx-3)" }}>search your people · one click</span>
                    </div>
                    <input className="input" placeholder="Search people you know to add…" value={pickerQ} onChange={(e) => setPickerQ(e.target.value)} />
                    {candidates.length === 0 ? (
                      <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", lineHeight: 1.6 }}>
                        {s.people.length === 0 ? "No one in your people yet — someone appears here once you share a group with them. Invite a new person below." : pickerQ ? "No match in your people. Invite them below, or add by address." : "Everyone you share a group with is already in this one — invite someone new below."}
                      </span>
                    ) : (
                      <div style={{ display: "flex", flexDirection: "column", gap: 2, maxHeight: 220, overflowY: "auto" }}>
                        {candidates.slice(0, 40).map((p) => {
                          const face = faceOf(p.address);
                          return (
                            <div key={p.address} style={{ display: "flex", alignItems: "center", gap: 10, padding: "6px 8px", borderRadius: "var(--r-1)" }}>
                              <span style={{ width: 26, height: 26, borderRadius: 999, background: "var(--srf-1)", border: "1px solid var(--line-2)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 10, fontWeight: 600, color: "var(--tx-2)", flexShrink: 0 }}>{initialsOf(face ? face.handle : p.address)}</span>
                              <span style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
                                <span style={{ fontSize: 12.5, fontWeight: 500 }}>{face ? `@${face.handle}` : shortAddr(p.address)}</span>
                                {p.groups.length > 0 && <span className="mono" style={{ fontSize: 9, color: "var(--tx-3)" }}>{p.groups.map((g) => g.name).slice(0, 2).join(", ")}</span>}
                              </span>
                              <button className="btn btn-secondary btn-sm" onClick={() => { if (!selected) return; void addMemberToGroup(selected.id, p.address); store.toast(`Adding ${face ? "@" + face.handle : shortAddr(p.address)} to the group…`); }}>Add</button>
                            </div>
                          );
                        })}
                      </div>
                    )}
                    <div style={{ height: 1, background: "var(--line-1)", margin: "2px 0" }} />
                    <span style={{ fontSize: 12, color: "var(--tx-2)" }}>Someone new? Use <strong>Invite someone</strong> below.</span>
                  </div>
                );
              })()}
              <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
                <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
                  <span style={{ fontSize: 13.5, fontWeight: 500 }}>Who's here</span>
                  <span className="mono" style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>{st.roster.length} member{st.roster.length === 1 ? "" : "s"}</span>
                </div>
                {canManage && !ROLE_CHANGES_AVAILABLE && st.roster.length > 0 && (
                  <p className="mono" style={{ fontSize: 10, color: "var(--tx-3)", margin: 0, padding: "8px 16px", borderBottom: "1px solid var(--line-1)" }}>{ROLE_CHANGES_NOT_AVAILABLE}</p>
                )}
                {st.roster.length === 0 ? (
                  <p style={{ fontSize: 12.5, color: "var(--tx-3)", margin: 0, padding: 16 }}>Just you so far. Add someone by their address below.</p>
                ) : (
                  st.roster.map((r) => {
                    const you = r.address.toLowerCase() === myAddr;
                    const isAgent = r.role === "agent";
                    const tone = ROLE_TONE[r.role];
                    const busy = st.busyMember === r.address;
                    return (
                      <div key={r.address} style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 16px", borderBottom: "1px solid var(--line-1)" }}>
                        <span className="mono" style={{ width: 30, height: 30, borderRadius: 999, border: isAgent ? "1px dashed var(--info)" : "1px solid var(--line-2)", background: you ? "var(--accent-wash)" : "#fff", color: "var(--tx-2)", display: "inline-flex", alignItems: "center", justifyContent: "center", fontSize: 10, fontWeight: 600, flexShrink: 0 }}>
                          {initialsOf(r.address)}
                        </span>
                        <span style={{ flex: 1, minWidth: 0 }}>
                          <span style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
                            <span style={{ fontSize: 13, fontWeight: 500 }}>{faceOf(r.address) ? "@" + faceOf(r.address)!.handle : shortAddr(r.address)}</span>
                            {faceOf(r.address) && <span className="mono" style={{ fontSize: 8.5, letterSpacing: ".06em", padding: "1px 6px", borderRadius: 999, border: "1px solid var(--ok)", color: "var(--ok)" }}>{faceOf(r.address)!.network} ✓</span>}
                            {you && <span className="mono" style={{ fontSize: 9, color: "var(--tx-3)" }}>you</span>}
                            {isAgent && <span className="mono" style={{ fontSize: 8.5, letterSpacing: ".1em", textTransform: "uppercase", padding: "1px 6px", borderRadius: 999, border: "1px dashed var(--info)", color: "var(--info)" }}>agent · keyless</span>}
                          </span>
                          <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", marginTop: 2, display: "block" }}>{r.address}</span>
                        </span>
                        <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".08em", textTransform: "uppercase", padding: "2px 9px", borderRadius: 999, border: "1px solid " + tone.bd, color: tone.fg, background: tone.bg }}>{r.role}</span>
                        {canManage && r.role !== "owner" && !you && !isAgent && (
                          <>
                            {ROLE_CHANGES_AVAILABLE ? (
                              <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => selected && doRole(selected.id, r.address, r.role)}>
                                {r.role === "admin" ? "Make member" : "Make admin"}
                              </button>
                            ) : (
                              <button className="btn btn-ghost btn-sm" disabled aria-disabled="true" title={ROLE_CHANGES_NOT_AVAILABLE} style={{ opacity: 0.5, cursor: "not-allowed" }}>
                                {r.role === "admin" ? "Make member" : "Make admin"}
                              </button>
                            )}
                            <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => selected && doOffboard(selected.id, r.address)} style={{ color: "var(--danger)" }}>Offboard</button>
                          </>
                        )}
                      </div>
                    );
                  })
                )}
                {/* Add-someone-you-know moved up into the prominent "Add people" card (#59). */}
                {/* Advanced fallback — add by raw comms address (for someone not yet in your people). */}
                {/* #61 find-via-X — find someone by their social handle (only people who opted into the
                     directory; self-published + revocable — a deliberate, consented D-7 exception). */}
                {canManage && (
                  <details style={{ borderTop: "1px solid var(--line-1)" }}>
                    <summary className="mono" style={{ fontSize: 10, color: "var(--tx-3)", padding: "10px 16px", cursor: "pointer" }}>Find someone by their X / Discord handle</summary>
                    <div style={{ display: "flex", gap: 8, padding: "0 16px 6px", alignItems: "center" }}>
                      {(["x", "discord"] as const).map((p) => (
                        <button key={p} className={"btn btn-sm " + (dirPlatform === p ? "btn-secondary" : "btn-ghost")} onClick={() => { setDirPlatform(p); setDirHits([]); }}>
                          {p === "x" ? "X" : "Discord"}
                        </button>
                      ))}
                      <input className="input" value={dirQ} placeholder={dirPlatform === "x" ? "@handle on X" : "username on Discord"} style={{ flex: 1 }} onChange={(e) => setDirQ(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void doDirectorySearch()} />
                      <button className="btn btn-secondary btn-sm" disabled={dirSearching} onClick={() => void doDirectorySearch()}>{dirSearching ? "…" : "Search"}</button>
                    </div>
                    {dirHits.map((h) => (
                      <div key={h.address} style={{ display: "flex", alignItems: "center", gap: 12, padding: "8px 16px" }}>
                        <span style={{ flex: 1, minWidth: 0 }}>
                          <span style={{ fontSize: 12.5, fontWeight: 500 }}>@{h.handle}{h.displayName ? <span style={{ color: "var(--tx-3)", fontWeight: 400 }}> · {h.displayName}</span> : null}</span>
                          <span className="mono" style={{ display: "block", fontSize: 9.5, color: "var(--tx-3)" }}>{shortAddr(h.address)} · self-published</span>
                        </span>
                        <button className="btn btn-primary btn-sm" onClick={() => { if (!selected) return; void addMemberToGroup(selected.id, h.address); store.toast(`Adding @${h.handle} to the group.`); }}>Add</button>
                      </div>
                    ))}
                    <p className="mono" style={{ fontSize: 10, color: "var(--tx-3)", margin: 0, padding: "4px 16px 12px", lineHeight: 1.5 }}>
                      Only shows people who opted in to being found by handle. Nobody else is ever resolved from a handle (D-7).
                    </p>
                  </details>
                )}
                {canManage && (
                  <details style={{ borderTop: "1px solid var(--line-1)" }}>
                    <summary className="mono" style={{ fontSize: 10, color: "var(--tx-3)", padding: "10px 16px", cursor: "pointer" }}>Add by address (advanced)</summary>
                    <div style={{ display: "flex", gap: 10, padding: "0 16px 12px", alignItems: "center" }}>
                      <input ref={addrRef} className="input" placeholder="0x comms address" style={{ flex: 1 }} onKeyDown={(e) => e.key === "Enter" && doAdd()} />
                      <button className="btn btn-secondary btn-sm" onClick={doAdd}>Add</button>
                    </div>
                  </details>
                )}
                {!canManage && (
                  <p className="mono" style={{ fontSize: 10, color: "var(--tx-3)", margin: 0, padding: "0 16px 12px" }}>
                    adding members needs the owner or an admin — the relay enforces this, not the UI
                  </p>
                )}
              </div>

              {/* D-12 — the inviter's side: Invite someone, and Pending (open links + requests). */}
              {canManage && selected && (
                <InvitePanel key={selected.id} store={store} groupId={selected.id} inviterName={store.identity().name} onAdded={() => void selectGroup(selected.id)} />
              )}
            </div>
          )}

          {/* ---- Cluster ---- */}
          {tab === "cluster" && (
            <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "14px 24px 20px", display: "flex", flexDirection: "column", gap: 14 }}>
              <div style={{ display: "grid", gridTemplateColumns: "repeat(3,1fr)", gap: 1, background: "var(--line-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-2)", overflow: "hidden" }}>
                <div style={{ background: "var(--srf-1)", padding: "12px 14px", display: "flex", flexDirection: "column", gap: 4 }}>
                  <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" }}>Cluster</span>
                  <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 16, color: joined ? (cl.status && cl.status.online > 0 ? "var(--ok)" : "var(--warn)") : "var(--tx-3)" }}>
                    {!joined ? "not joined" : cl.status && cl.status.online > 0 ? "healthy" : "waiting for peers"}
                  </span>
                  <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>{joined ? "your node contributes storage" : "join to hold the group's data"}</span>
                </div>
                <div style={{ background: "var(--srf-1)", padding: "12px 14px", display: "flex", flexDirection: "column", gap: 4 }}>
                  <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" }}>Peers</span>
                  <span className="tabular" style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 21 }}>{cl.status ? `${cl.status.online}/${cl.status.total}` : "—"}</span>
                  <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>online / holding the data</span>
                </div>
                <div style={{ background: "var(--srf-1)", padding: "12px 14px", display: "flex", flexDirection: "column", gap: 4 }}>
                  <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" }}>Co-pinned</span>
                  <span className="tabular" style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 21 }}>{cl.status?.sharedFiles.length ?? 0}</span>
                  <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>files held by the roster</span>
                </div>
              </div>

              {clErr && <div role="alert" style={{ fontSize: 12, color: "var(--danger)", lineHeight: 1.5 }}>{clErr}</div>}

              {!joined ? (
                <div className="surface" style={{ padding: 18, display: "flex", alignItems: "center", gap: 14 }}>
                  <span style={{ flex: 1 }}>
                    <span style={{ display: "block", fontSize: 13.5, fontWeight: 500 }}>You're not in this cluster yet.</span>
                    <span style={{ display: "block", fontSize: 12.5, color: "var(--tx-2)", marginTop: 3, lineHeight: 1.55 }}>
                      Joining contributes your storage and keeps you in sync — the group's shared files survive any one member going offline.
                    </span>
                  </span>
                  <button className="btn btn-primary" disabled={cl.joining} onClick={() => selected && void joinCluster(selected.id)}>{cl.joining ? "Joining…" : "Join cluster"}</button>
                </div>
              ) : (
                <>
                  {/* peers */}
                  <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
                    <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
                      <span style={{ fontSize: 13.5, fontWeight: 500 }}>Peers</span>
                      <button className="btn btn-ghost btn-sm" onClick={() => selected && void leaveCluster(selected.id)} style={{ marginLeft: "auto", color: "var(--tx-3)" }}>Leave cluster</button>
                    </div>
                    {cl.peers.length === 0 ? (
                      <p style={{ fontSize: 12.5, color: "var(--tx-3)", margin: 0, padding: 16, lineHeight: 1.6 }}>
                        {cl.loading ? "Loading…" : "No peers connected yet — a lone node meshes with no one until other members come online."}
                      </p>
                    ) : (
                      cl.peers.map((p) => {
                        const isAgent = st.roster.find((r) => r.address.toLowerCase() === p.address.toLowerCase())?.role === "agent";
                        return (
                          <div key={p.address} style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 16px", borderBottom: "1px solid var(--line-1)" }}>
                            <span style={{ width: 7, height: 7, borderRadius: 999, background: p.online ? "var(--ok)" : "var(--tx-3)", flexShrink: 0 }}></span>
                            <span style={{ flex: 1, minWidth: 0 }}>
                              <span style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
                                <span style={{ fontSize: 12.5, fontWeight: 500 }}>{faceOf(p.address) ? "@" + faceOf(p.address)!.handle : shortAddr(p.address)}</span>
                                {isAgent && <span className="mono" style={{ fontSize: 8.5, letterSpacing: ".1em", textTransform: "uppercase", padding: "1px 6px", borderRadius: 999, border: "1px dashed var(--info)", color: "var(--info)" }}>agent</span>}
                              </span>
                              <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)" }}>{p.online ? "online" : "authorized · offline"}</span>
                            </span>
                          </div>
                        );
                      })
                    )}
                  </div>

                  {/* share a file — drag / Browse / pick from your files (NO raw CID for normal users) */}
                  <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
                    <div style={{ padding: "12px 16px", borderBottom: "1px solid var(--line-1)", fontSize: 13.5, fontWeight: 500 }}>Share a file with the group</div>
                    <div
                      style={{ margin: 16, padding: "22px 18px", textAlign: "center", border: "2px dashed " + (dropOver ? "var(--accent)" : "var(--line-2)"), borderRadius: "var(--r-2)", background: dropOver ? "var(--accent-wash)" : "transparent", transition: "border-color .12s, background .12s" }}
                    >
                      <div style={{ fontSize: 13.5, fontWeight: 520 }}>{dropOver ? "Drop to share" : "Drag a file here to share it"}</div>
                      <div style={{ fontSize: 11.5, color: "var(--tx-3)", marginTop: 6, lineHeight: 1.5 }}>
                        The whole group co-pins it, so your shared data survives any one of you going offline.
                      </div>
                      <button className="btn btn-secondary btn-sm" style={{ marginTop: 12 }} onClick={() => void browseAndShare()}>Browse files…</button>
                    </div>

                    <div style={{ padding: "0 16px 6px", display: "flex", alignItems: "center", gap: 8 }}>
                      <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>Or share one of your files</span>
                    </div>
                    {cl.myFiles.length === 0 ? (
                      <p style={{ fontSize: 12, color: "var(--tx-3)", margin: 0, padding: "0 16px 14px", lineHeight: 1.6 }}>
                        No files on your node yet. Add some in Files, or drag one onto the box above.
                      </p>
                    ) : (
                      cl.myFiles.map((f) => {
                        const already = cl.status?.sharedFiles.includes(f.cid);
                        return (
                          <div key={f.cid} style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 16px", borderTop: "1px solid var(--line-1)" }}>
                            <span style={{ flex: 1, minWidth: 0 }}>
                              <span className="mono" style={{ display: "block", fontSize: 11.5, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{f.cid}</span>
                              <span className="mono" style={{ display: "block", fontSize: 9.5, color: "var(--tx-3)", marginTop: 1 }}>{humanBytes(f.sizeBytes)} · {f.pinState}</span>
                            </span>
                            {already ? (
                              <span className="mono" style={{ fontSize: 10, color: "var(--ok)" }}>shared</span>
                            ) : (
                              <button className="btn btn-ghost btn-sm" disabled={cl.sharing === f.cid} onClick={() => selected && void shareClusterFile(selected.id, f.cid)}>
                                {cl.sharing === f.cid ? "…" : "Share"}
                              </button>
                            )}
                          </div>
                        );
                      })
                    )}

                    {/* advanced: paste a CID (power users) */}
                    <div style={{ borderTop: "1px solid var(--line-1)", padding: "10px 16px" }}>
                      {!cidAdvanced ? (
                        <button onClick={() => setCidAdvanced(true)} style={{ fontFamily: "var(--font-mono)", fontSize: 10, color: "var(--tx-3)", background: "none", border: "none", cursor: "pointer", padding: 0, textDecoration: "underline" }}>
                          Advanced: paste a CID
                        </button>
                      ) : (
                        <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
                          <input ref={cidRef} className="input mono" placeholder="bafy…" style={{ flex: 1 }} />
                          <button className="btn btn-secondary btn-sm" onClick={() => { const c = cidRef.current?.value.trim() ?? ""; if (c && selected) void shareClusterFile(selected.id, c); if (cidRef.current) cidRef.current.value = ""; }}>Share CID</button>
                        </div>
                      )}
                    </div>
                    <p className="mono" style={{ fontSize: 10, color: "var(--tx-3)", margin: 0, padding: "0 16px 12px" }}>a bonded share stops at the ceremony before any SALT is committed</p>
                  </div>
                </>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
