// =====================================================================
// citrate-core — Groups (CX-S3 redesign, Pass 1)
//
// The social spine, built 1:1 from design/CitrateCore.dc.html. A left rail (create / join / list)
// and a selected group with three tabs: Conversation, Roster, Cluster. RBAC is enforced at the
// RELAY, not the client (ADR-001) — the surface offers controls and shows the relay's honest error
// if you are not authorized; it never fakes success (Rule 1). Role changes and offboard stop at the
// Signature Ceremony. The Cluster tab shares FILES (drag / Browse / pick from your files), never a
// hand-typed CID — normal people don't have a CID to paste.
// =====================================================================
import { useEffect, useRef, useState } from "react";
import { SurfaceProps } from "./shared";
import { bridge } from "../bridge";
import type { Group, GroupRole, ResolvedIdentity } from "../bridge/domains";
import {
  groupsSlice,
  refreshGroups,
  createGroup,
  selectGroup,
  sendMessage,
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

  const selected = st.groups.find((g) => g.id === st.selectedId) ?? null;
  const myWallet = typeof store.identity === "function" ? store.identity().wallet : "";
  const myAddr = (myWallet || s.walletAddr || "").toLowerCase();
  const myRole = st.roster.find((r) => r.address.toLowerCase() === myAddr)?.role ?? (selected && selected.owner.toLowerCase() === myAddr ? "owner" : "member");
  const canManage = myRole === "owner" || myRole === "admin";

  // Verified, group-visible faces for the addresses on screen (ADR resolver). Self today;
  // cross-member when bindings are shared server-blind to groups. Fallback is the address avatar.
  const [faces, setFaces] = useState<Record<string, ResolvedIdentity>>({});
  const faceOf = (addr: string): ResolvedIdentity | undefined => faces[(addr || "").toLowerCase()];

  useEffect(() => {
    void refreshGroups();
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
      store.toast("Handles resolve once Social discovery ships — add a 0x address for now.");
      return;
    }
    if (addrRef.current) addrRef.current.value = "";
    void addMemberToGroup(v);
  };
  const doRole = (address: string, role: GroupRole) => {
    const next: GroupRole = role === "admin" ? "member" : "admin";
    void store.requestSig({
      origin: "user wallet action",
      requester: "you · group admin",
      title: next === "admin" ? "Make this member an admin" : "Return this admin to member",
      rows: [
        { k: "Member", v: shortAddr(address) },
        { k: "New role", v: next },
        { k: "Effect", v: "a signed RoleAssertion, enforced at the relay" },
      ],
      cost: "—",
      sponsor: "you approve · one assertion",
      sponsorColor: "var(--ok)",
      chainless: true,
      apply: () => void assignRole(address, next),
    });
  };
  const doOffboard = (address: string) => {
    void store.requestSig({
      origin: "user wallet action",
      requester: "you · group admin",
      title: "Offboard this member",
      rows: [
        { k: "Member", v: shortAddr(address) },
        { k: "Effect", v: "removed from messaging, roster, cluster, and roles" },
      ],
      cost: "—",
      sponsor: "you approve · one action",
      sponsorColor: "var(--danger)",
      chainless: true,
      warning: "This removes them across all planes in one epoch and rotates the group secret — they lose access everywhere at once. This cannot be undone.",
      apply: () => {
        void offboardMember(address);
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
    <div style={{ display: "grid", gridTemplateColumns: "248px minmax(0,1fr)", minHeight: "100%", boxSizing: "border-box" }}>
      {/* ---------- left rail ---------- */}
      <div style={{ borderRight: "1px solid var(--line-1)", background: "var(--srf-1)", padding: "18px 14px", display: "flex", flexDirection: "column", gap: 12, minHeight: 0, overflow: "auto" }}>
        <div style={{ display: "flex", alignItems: "center" }}>
          <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 19 }}>Groups</span>
          <button className="btn btn-ghost btn-sm" onClick={() => setCreateOpen((v) => !v)} style={{ marginLeft: "auto" }}>
            {createOpen ? "Close" : "New"}
          </button>
        </div>

        {createOpen && (
          <div style={{ display: "flex", flexDirection: "column", gap: 8, border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: 12, background: "#fff" }}>
            <span className="lbl">New group</span>
            <input ref={nameRef} className="input" placeholder="Name" onKeyDown={(e) => e.key === "Enter" && doCreate()} />
            <div style={{ display: "flex", gap: 6 }}>
              {KINDS.map((k) => (
                <button key={k.id} className={"btn btn-sm " + (kind === k.id ? "btn-secondary" : "btn-ghost")} onClick={() => setKind(k.id)}>
                  {k.label}
                </button>
              ))}
            </div>
            <span className="mono" style={{ fontSize: 9.5, color: "var(--tx-3)", lineHeight: 1.5 }}>{KINDS.find((k) => k.id === kind)!.note}</span>
            <button className="btn btn-primary btn-sm" onClick={doCreate} disabled={st.creating}>{st.creating ? "Creating…" : "Create group"}</button>
            <div style={{ borderTop: "1px solid var(--line-1)", paddingTop: 10, display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="lbl">Or join by id</span>
              <div style={{ display: "flex", gap: 6 }}>
                <input ref={joinRef} className="input" placeholder="grp_…" style={{ flex: 1, minWidth: 0 }} onKeyDown={(e) => e.key === "Enter" && doJoin()} />
                <button className="btn btn-secondary btn-sm" onClick={doJoin}>Join</button>
              </div>
            </div>
          </div>
        )}

        {st.error && (
          <div role="alert" style={{ fontSize: 11.5, color: "var(--danger)", lineHeight: 1.5, padding: "2px 2px" }}>{st.error}</div>
        )}

        {st.groups.length === 0 ? (
          <p style={{ fontSize: 12, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: "4px 2px" }}>
            No groups yet. Create one to bring people together, or join one you've been pointed to.
          </p>
        ) : (
          <ul style={{ listStyle: "none", padding: 0, margin: 0, display: "flex", flexDirection: "column", gap: 2 }}>
            {st.groups.map((g) => {
              const on = g.id === st.selectedId;
              return (
                <li key={g.id}>
                  <a
                    href="#/groups"
                    onClick={(e) => { e.preventDefault(); setTab("chat"); void selectGroup(g.id); }}
                    style={{ display: "flex", alignItems: "center", gap: 10, padding: "9px 10px", borderRadius: "var(--r-1)", textDecoration: "none", background: on ? "var(--srf-2)" : "transparent" }}
                  >
                    <span className="mono" style={{ width: 26, height: 26, borderRadius: "var(--r-1)", border: "1px solid var(--line-2)", background: "#fff", color: "var(--tx-2)", display: "inline-flex", alignItems: "center", justifyContent: "center", fontSize: 10, fontWeight: 600, flexShrink: 0 }}>
                      {initialsOf(groupLabel(st, g))}
                    </span>
                    <span style={{ flex: 1, minWidth: 0 }}>
                      <span style={{ display: "block", fontSize: 12.5, fontWeight: on ? 500 : 400, color: "var(--tx-1)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{groupLabel(st, g)}</span>
                      <span className="mono" style={{ display: "block", fontSize: 9.5, color: "var(--tx-3)" }}>
                        {KINDS.find((k) => k.id === g.kind)?.label ?? g.kind}{g.members.length ? ` · ${g.members.length}` : ""}
                      </span>
                    </span>
                  </a>
                </li>
              );
            })}
          </ul>
        )}
      </div>

      {/* ---------- main ---------- */}
      {!selected ? (
        <div className="lattice-dots" style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 14, padding: 48, textAlign: "center" }}>
          <div style={{ fontFamily: "var(--font-display)", fontWeight: 400, fontSize: 26 }}>Your groups live here.</div>
          <p style={{ fontSize: 13.5, lineHeight: 1.6, color: "var(--tx-2)", margin: 0, maxWidth: 460 }}>
            A group is a space with real roles — enforced at the relay, which only ever sees ciphertext. Reach stays in your groups. Talk, hold files together as a cluster, and grow it toward the network's milestones.
          </p>
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", minHeight: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "16px 24px 0", flexWrap: "wrap" }}>
            <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 22, minWidth: 0, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", maxWidth: "100%" }}>{groupLabel(st, selected)}</span>
            <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", padding: "2px 9px", borderRadius: 999, border: "1px solid var(--line-2)", color: "var(--tx-2)", flexShrink: 0 }}>
              {KINDS.find((k) => k.id === selected.kind)?.label ?? selected.kind}
            </span>
            <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", flexShrink: 0 }}>{selected.id.slice(0, 12)}…</span>
            <span style={{ display: "flex", gap: 2, background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: 2, marginLeft: "auto", flexShrink: 0 }}>
              {([["chat", "Conversation"], ["roster", "Roster"], ["cluster", "Cluster"]] as const).map(([id, label]) => {
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
              <div className="surface" style={{ flex: 1, minHeight: 280, overflow: "auto", padding: 16, display: "flex", flexDirection: "column", gap: 14 }}>
                {st.messages.length === 0 ? (
                  <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-3)", margin: "auto", textAlign: "center", maxWidth: 340 }}>
                    Nothing here yet. Say hello — messages are relayed to every member and readable by the roles you see in the roster.
                  </p>
                ) : (
                  st.messages.map((m) => {
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
                          <span style={{ fontSize: 13, lineHeight: 1.55, color: "var(--tx-1)", whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{m.body}</span>
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

          {/* ---- Roster ---- */}
          {tab === "roster" && (
            <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "14px 24px 20px", display: "flex", flexDirection: "column", gap: 14 }}>
              <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
                <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
                  <span style={{ fontSize: 13.5, fontWeight: 500 }}>Roster</span>
                  <span className="mono" style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>{st.roster.length} member{st.roster.length === 1 ? "" : "s"}</span>
                </div>
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
                            <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => doRole(r.address, r.role)} title="Signed RoleAssertion — stops at the ceremony">
                              {r.role === "admin" ? "Make member" : "Make admin"}
                            </button>
                            <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => doOffboard(r.address)} style={{ color: "var(--danger)" }}>Offboard</button>
                          </>
                        )}
                      </div>
                    );
                  })
                )}
                <div style={{ display: "flex", gap: 10, padding: "12px 16px", alignItems: "center" }}>
                  <input ref={addrRef} className="input" placeholder="0x address or @handle — handles resolve via Social discovery" style={{ flex: 1 }} disabled={!canManage} onKeyDown={(e) => e.key === "Enter" && canManage && doAdd()} />
                  <button className="btn btn-secondary btn-sm" onClick={doAdd} disabled={!canManage}>Add member</button>
                </div>
                {!canManage && (
                  <p className="mono" style={{ fontSize: 10, color: "var(--tx-3)", margin: 0, padding: "0 16px 12px" }}>
                    adding members needs the owner or an admin — the relay enforces this, not the UI
                  </p>
                )}
              </div>
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
