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
import type { Group, GroupRole, InviteClaim, PendingInvite, ResolvedIdentity } from "../bridge/domains";
import { SOCIAL_BINDING_MSG_PREFIX } from "../bridge/domains";
import { addablePeople, filterPeople } from "./peopleDirectory";
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
  // D4 claimable invites
  const inviteHandleRef = useRef<HTMLInputElement>(null);
  const claimRef = useRef<HTMLInputElement>(null);
  const redeemRef = useRef<HTMLInputElement>(null);
  const [pendingInvites, setPendingInvites] = useState<PendingInvite[]>([]);
  const [requests, setRequests] = useState<InviteClaim[]>([]);
  const [redeemOpen, setRedeemOpen] = useState(false);
  const [pickerQ, setPickerQ] = useState(""); // CONNECT-S2 — add-member people-picker search

  const selected = st.groups.find((g) => g.id === st.selectedId) ?? null;
  const myWallet = typeof store.identity === "function" ? store.identity().wallet : "";
  const myAddr = (myWallet || s.walletAddr || "").toLowerCase();
  const myRole = st.roster.find((r) => r.address.toLowerCase() === myAddr)?.role ?? (selected && selected.owner.toLowerCase() === myAddr ? "owner" : "member");
  // A group you created THIS session is yours to manage, even if the daemon keys your roster seat
  // by a different address than your wallet (a known addressing seam). RBAC is still enforced at the
  // relay — this only decides which controls the UI offers, never whether an action is authorized.
  const iCreated = !!(selected && st.names[selected.id]);
  const canManage = myRole === "owner" || myRole === "admin" || iCreated;

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

  // Control messages never render in the conversation.
  const visibleMessages = st.messages.filter((m) => !m.body.startsWith(SOCIAL_BINDING_MSG_PREFIX));

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
      store.toast("To invite by @handle, use “Invite by @handle” below — Citrate never looks up an address from a handle.");
      return;
    }
    if (addrRef.current) addrRef.current.value = "";
    void addMemberToGroup(v);
  };

  // D4 claimable invites (owner side)
  const refreshInvites = async () => {
    if (!selected || !canManage) return;
    try {
      setPendingInvites(await bridge.invites.list(selected.id));
    } catch {
      /* honest: none / relay unavailable */
    }
    // CONNECT-S1 — poll the server-blind claims-inbox for incoming requests (no DM-back). Honest-empty
    // if the relay/daemon can't be reached; never a fabricated request (Rule 1).
    try {
      setRequests(await bridge.invites.pollClaims(selected.id));
    } catch {
      setRequests([]);
    }
  };
  // CONNECT-S1 — approve an incoming request: consume the one-time token, then add via the normal path.
  const approveRequest = async (req: { group: string; token: string; address: string }) => {
    if (!selected) return;
    try {
      const ok = await bridge.invites.verifyConsume(selected.id, req.token);
      if (!ok) {
        store.toast("That request's invite is invalid or already used.");
        await refreshInvites();
        return;
      }
      void addMemberToGroup(req.address);
      store.toast("Request approved — adding them to the group.");
      await refreshInvites();
    } catch (e) {
      store.toast(e instanceof Error ? e.message : String(e));
    }
  };
  useEffect(() => {
    if (tab === "roster" && selected && canManage) {
      void refreshInvites();
      void store.refreshPeople(); // CONNECT-S2 — keep the add-member picker's people fresh
    } else setPendingInvites([]);
  }, [tab, selected?.id, canManage]);

  const doMintInvite = async () => {
    if (!selected) return;
    const h = (inviteHandleRef.current?.value.trim() ?? "").replace(/^@/, "");
    if (!h) {
      store.toast("Enter the @handle you're inviting.");
      return;
    }
    try {
      const { link } = await bridge.invites.create(selected.id, h);
      if (inviteHandleRef.current) inviteHandleRef.current.value = "";
      try {
        await navigator.clipboard?.writeText(link);
      } catch {
        /* clipboard may be unavailable */
      }
      store.toast(`Invite for @${h} copied — DM it to them on the platform. They open it, accept, and send you their claim to paste below.`);
      await refreshInvites();
    } catch (e) {
      store.toast(e instanceof Error ? e.message : String(e));
    }
  };
  const doAddFromClaim = async () => {
    const raw = claimRef.current?.value.trim() ?? "";
    if (!raw || !selected) return;
    let claim: { group?: string; token?: string; address?: string };
    try {
      claim = JSON.parse(raw);
    } catch {
      store.toast("That claim isn't valid — paste the whole claim they sent back.");
      return;
    }
    if (claim.group !== selected.id || !claim.token || !claim.address) {
      store.toast("That claim is for a different group or is incomplete.");
      return;
    }
    try {
      const ok = await bridge.invites.verifyConsume(selected.id, claim.token);
      if (!ok) {
        store.toast("That invite token is invalid or already used.");
        return;
      }
      if (claimRef.current) claimRef.current.value = "";
      void addMemberToGroup(claim.address);
      store.toast("Invite accepted — adding them to the group.");
      await refreshInvites();
    } catch (e) {
      store.toast(e instanceof Error ? e.message : String(e));
    }
  };
  // Invitee side: turn an invite link into a request. CONNECT-S1 — a link with a `k=` key seals the
  // claim and submits it to the owner over the server-blind relay (one click, no DM-back). An older
  // link (no key) falls back to the copy-the-claim path so it still works.
  const doRedeemLink = async () => {
    const link = redeemRef.current?.value.trim() ?? "";
    if (!link) return;
    const g = /[?&]g=([^&]*)/.exec(link)?.[1];
    const t = /[?&]t=([^&]*)/.exec(link)?.[1];
    const k = /[?&]k=([^&]*)/.exec(link)?.[1];
    if (!g || !t) {
      store.toast("That doesn't look like an invite link.");
      return;
    }
    if (k) {
      // CONNECT-S1 one-click: seal + submit the request to the relay's inbox.
      try {
        await bridge.invites.submitClaim(link);
        if (redeemRef.current) redeemRef.current.value = "";
        setRedeemOpen(false);
        store.toast("Request sent — the person who invited you will see it and approve you. No copy-paste needed.");
      } catch (e) {
        store.toast("Couldn't send the request — " + (e instanceof Error ? e.message : String(e)));
      }
      return;
    }
    // Pre-S1 link (no key): fall back to the manual claim.
    const address = myWallet || s.walletAddr || "";
    if (!address) {
      store.toast("Your wallet isn't ready yet — try again once it's provisioned.");
      return;
    }
    const claim = JSON.stringify({ group: g, token: t, address });
    try {
      void navigator.clipboard?.writeText(claim);
    } catch {
      /* clipboard may be unavailable */
    }
    if (redeemRef.current) redeemRef.current.value = "";
    setRedeemOpen(false);
    store.toast("This is an older invite — claim copied; DM it back to whoever invited you.");
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
        {/* D4 (invitee) — turn an invite link you were DM'd into a claim to send back. */}
        <div style={{ marginTop: "auto", borderTop: "1px solid var(--line-1)", paddingTop: 10 }}>
          {!redeemOpen ? (
            <button onClick={() => setRedeemOpen(true)} style={{ fontFamily: "var(--font-mono)", fontSize: 10, color: "var(--tx-3)", background: "none", border: "none", cursor: "pointer", padding: "2px", textDecoration: "underline" }}>
              Have an invite link?
            </button>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="lbl">Redeem an invite</span>
              <input ref={redeemRef} className="input mono" placeholder="citrate://invite?…" onKeyDown={(e) => e.key === "Enter" && doRedeemLink()} />
              <button className="btn btn-secondary btn-sm" onClick={doRedeemLink}>Accept &amp; copy my claim</button>
              <span className="mono" style={{ fontSize: 9, color: "var(--tx-3)", lineHeight: 1.5 }}>copies a claim with your address to DM back — nobody looks up your address</span>
            </div>
          )}
        </div>
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
                {/* CONNECT-S2 — the people-picker: add someone you already share a group with, one click,
                    no address to paste. Sourced from your People directory minus this group's roster. */}
                {canManage && (() => {
                  const candidates = filterPeople(addablePeople(s.people, st.roster.map((r) => r.address)), pickerQ);
                  return (
                    <div style={{ borderTop: "1px solid var(--line-1)", padding: "12px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                        <span style={{ fontSize: 13.5, fontWeight: 500 }}>Add someone you know</span>
                        <span className="mono" style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".08em", textTransform: "uppercase", color: "var(--tx-3)" }}>from your people · one click</span>
                      </div>
                      <input className="input" placeholder="Search your people to add…" value={pickerQ} onChange={(e) => setPickerQ(e.target.value)} />
                      {candidates.length === 0 ? (
                        <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", lineHeight: 1.6 }}>
                          {s.people.length === 0 ? "No people yet — someone appears here once you share a group with them." : "Everyone you share a group with is already in this one."}
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
                                <button className="btn btn-secondary btn-sm" onClick={() => { void addMemberToGroup(p.address); store.toast(`Adding ${face ? "@" + face.handle : shortAddr(p.address)} to the group…`); }}>Add</button>
                              </div>
                            );
                          })}
                        </div>
                      )}
                    </div>
                  );
                })()}
                {/* Advanced fallback — add by raw comms address (for someone not yet in your people). */}
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

              {/* D4 — claimable invite by @handle (owner/admin) */}
              {canManage && (
                <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
                  <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
                    <span style={{ fontSize: 13.5, fontWeight: 500 }}>Invite by @handle</span>
                    <span className="mono" style={{ marginLeft: "auto", fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>no address lookup</span>
                  </div>
                  <div style={{ display: "flex", gap: 10, padding: "12px 16px", alignItems: "center" }}>
                    <input ref={inviteHandleRef} className="input" placeholder="@handle on X, Discord, …" style={{ flex: 1 }} onKeyDown={(e) => e.key === "Enter" && void doMintInvite()} />
                    <button className="btn btn-secondary btn-sm" onClick={() => void doMintInvite()}>Create invite</button>
                  </div>
                  {pendingInvites.map((pi) => (
                    <div key={pi.token} style={{ display: "flex", alignItems: "center", gap: 12, padding: "8px 16px", borderTop: "1px solid var(--line-1)" }}>
                      <span style={{ flex: 1, minWidth: 0 }}>
                        <span style={{ fontSize: 12.5, fontWeight: 500 }}>@{pi.forHandle}</span>
                        <span className="mono" style={{ display: "block", fontSize: 9.5, color: "var(--tx-3)" }}>invite pending · claimable once</span>
                      </span>
                      <button className="btn btn-ghost btn-sm" onClick={() => { void navigator.clipboard?.writeText(pi.link || `citrate://invite?g=${pi.group}&t=${pi.token}`); store.toast("Invite link copied — DM it to them."); }}>Copy link</button>
                      <button className="btn btn-ghost btn-sm" style={{ color: "var(--tx-3)" }} onClick={() => void bridge.invites.revoke(pi.group, pi.token).then(refreshInvites)}>Revoke</button>
                    </div>
                  ))}
                  {/* CONNECT-S1 — incoming requests (server-blind inbox): approve in one click, no paste. */}
                  {requests.length > 0 && (
                    <div style={{ borderTop: "1px solid var(--line-1)" }}>
                      <div style={{ padding: "10px 16px 4px", fontSize: 11, letterSpacing: ".04em", textTransform: "uppercase", color: "var(--accent-text)" }}>
                        Requests · {requests.length}
                      </div>
                      {requests.map((req) => {
                        const face = faceOf(req.address);
                        return (
                          <div key={req.token + req.address} style={{ display: "flex", alignItems: "center", gap: 12, padding: "8px 16px" }}>
                            <span style={{ width: 28, height: 28, borderRadius: 999, background: "var(--srf-1)", border: "1px solid var(--line-2)", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 10.5, fontWeight: 600, color: "var(--tx-2)", flexShrink: 0 }}>{initialsOf(face ? face.handle : req.address)}</span>
                            <span style={{ flex: 1, minWidth: 0 }}>
                              <span style={{ fontSize: 12.5, fontWeight: 500 }}>{face ? `@${face.handle}` : shortAddr(req.address)}</span>
                              <span className="mono" style={{ display: "block", fontSize: 9.5, color: "var(--tx-3)" }}>wants to join · {shortAddr(req.address)}</span>
                            </span>
                            <button className="btn btn-primary btn-sm" onClick={() => void approveRequest(req)}>Approve</button>
                          </div>
                        );
                      })}
                    </div>
                  )}
                  {/* Fallback for older invite links (no sealed key): the manual claim paste. */}
                  <details style={{ borderTop: "1px solid var(--line-1)" }}>
                    <summary className="mono" style={{ fontSize: 10, color: "var(--tx-3)", padding: "10px 16px", cursor: "pointer" }}>Older invite? Paste a claim manually</summary>
                    <div style={{ display: "flex", gap: 10, padding: "0 16px 12px", alignItems: "center" }}>
                      <input ref={claimRef} className="input mono" placeholder="Paste the claim they DM'd back" style={{ flex: 1 }} />
                      <button className="btn btn-secondary btn-sm" onClick={() => void doAddFromClaim()}>Accept claim</button>
                    </div>
                  </details>
                  <p className="mono" style={{ fontSize: 10, color: "var(--tx-3)", margin: 0, padding: "0 16px 12px", lineHeight: 1.6 }}>
                    Citrate never resolves a handle to an address. You DM the invite link; when they open it their client sends you a request here (server-blind) — you approve it and they join. Their address is their consent.
                  </p>
                </div>
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
