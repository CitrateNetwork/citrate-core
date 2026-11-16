// =====================================================================
// citrate-core — Groups › Members (0.5.4 FL design, board 03)
//
// One group's people: who they are, their devices and whether they are connected, their role, and
// what you may change. A role change is a signed role grant (#637): the comms-member-daemon signs it
// with this device's comms key (not the wallet) and every member checks it. Each change and each
// offboard stops at the review sheet first (HIC-1). Invites are single-use links (InvitePanel).
//
// Data sources (Rule 7):
//   roster and roles     comms-member-daemon `groups_roster` (with signed role grants applied)
//   role change          comms-member-daemon `grantRole` via `groups_assign_role`
//   offboard             comms-member-daemon `groups_offboard` (all planes, one epoch)
//   devices and presence cluster daemon `cluster_devices` (live); "—" with the reason when unread
//   faces               verified social bindings (`social.resolve`); else a short address
//   invites              src-tauri invites.rs via InvitePanel
// Nothing here is invented: an unread value shows "—" and says why.
// =====================================================================
import { useEffect, useMemo, useState } from "react";
import type { SurfaceProps } from "../shared";
import { bridge } from "../../bridge";
import type { ClusterMemberDevices, DirectorySearchHit, GroupRole, ResolvedIdentity } from "../../bridge/domains";
import { InvitePanel } from "../InvitePanel";
import { addablePeople, filterPeople } from "../peopleDirectory";
import {
  groupsSlice,
  refreshGroups,
  selectGroup,
  assignRole,
  offboardMember,
  addMemberToGroup,
  groupLabel,
} from "../../shell/slices/groups";
import { canInvite, ceremonyTitle, myRoleIn, presenceOf, roleActions, roleWord, type RoleAction } from "./memberRoles";
import "./groups.css";

const shortAddr = (a: string): string => (!a ? "—" : a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a);

const ROLE_TONE: Record<string, string> = {
  owner: "var(--accent-text)",
  admin: "var(--info)",
  member: "var(--tx-2)",
  guest: "var(--tx-3)",
  agent: "var(--info)",
};

/** A role change this device signed and sent, until the roster shows it. */
interface Applying {
  address: string;
  to: GroupRole | "removed";
}

export function GroupMembers({ store, s }: SurfaceProps) {
  const st = groupsSlice.use();
  const selected = st.groups.find((g) => g.id === st.selectedId) ?? null;
  const [faces, setFaces] = useState<Record<string, ResolvedIdentity>>({});
  const [devices, setDevices] = useState<ClusterMemberDevices[] | null>(null);
  const [devicesWhy, setDevicesWhy] = useState("Reading the group's cluster.");
  const [rosterRead, setRosterRead] = useState(false);
  const [applying, setApplying] = useState<Applying | null>(null);
  const [pickerQ, setPickerQ] = useState("");
  const [dirPlatform, setDirPlatform] = useState<"x" | "discord">("x");
  const [dirQ, setDirQ] = useState("");
  const [dirHits, setDirHits] = useState<DirectorySearchHit[]>([]);
  const [addr, setAddr] = useState("");

  useEffect(() => {
    void refreshGroups();
    void store.refreshPeople();
  }, []);

  // Open on a group: the one selected in Groups, else the first one you are in.
  useEffect(() => {
    if (!st.selectedId && st.groups.length > 0) void selectGroup(st.groups[0].id);
  }, [st.selectedId, st.groups.length]);

  // The roster for the selected group has been read once it lands (an empty roster is "not read").
  useEffect(() => {
    setRosterRead(st.roster.length > 0);
  }, [st.roster, st.selectedId]);

  // Devices and presence come from the cluster daemon. A failed read is "—" with its reason.
  useEffect(() => {
    if (!selected) return;
    let live = true;
    setDevices(null);
    setDevicesWhy("Reading the group's cluster.");
    bridge.cluster
      .devices(selected.id)
      .then((d) => live && setDevices(d))
      .catch((e) => live && setDevicesWhy("The group's cluster did not answer: " + (e instanceof Error ? e.message : String(e))));
    return () => {
      live = false;
    };
  }, [selected?.id]);

  useEffect(() => {
    const addrs = st.roster.map((r) => r.address).filter(Boolean);
    if (!addrs.length) return;
    let live = true;
    bridge.social
      .resolve(addrs)
      .then((res) => {
        if (!live) return;
        const m: Record<string, ResolvedIdentity> = {};
        for (const r of res) m[r.address.toLowerCase()] = r;
        setFaces(m);
      })
      .catch(() => {
        /* no faces: the short address shows */
      });
    return () => {
      live = false;
    };
  }, [st.roster]);

  // A grant is applied once the roster shows it (or shows the member gone, for an offboard).
  useEffect(() => {
    if (!applying) return;
    const seat = st.roster.find((r) => r.address.toLowerCase() === applying.address.toLowerCase());
    if ((applying.to === "removed" && !seat) || (seat && seat.role === applying.to)) setApplying(null);
  }, [st.roster, applying]);

  const myWallet = typeof store.identity === "function" ? store.identity().wallet : "";
  const me = (s.commsAddr || myWallet || s.walletAddr || "").toLowerCase();
  const navRole = selected ? s.myGroups.find((g) => g.id === selected.id)?.myRole : null;
  const myRole = selected ? myRoleIn(navRole, st.roster, selected.owner, me) : null;
  const manage = canInvite(myRole);
  const nameOf = (a: string): string => {
    if (a.toLowerCase() === me) return "You";
    const f = faces[a.toLowerCase()];
    return f ? "@" + f.handle : shortAddr(a);
  };
  const managers = st.roster.filter((r) => r.role === "owner" || r.role === "admin").map((r) => nameOf(r.address));

  const stillOn = (groupId: string): boolean => {
    if (groupsSlice.get().selectedId === groupId) return true;
    store.toast("The selected group changed after this was proposed, so nothing was sent. Open that group and try again.");
    return false;
  };

  const ask = (groupId: string, groupName: string, address: string, a: RoleAction) => {
    const who = nameOf(address);
    const off = a.to === "removed";
    void store.requestSig({
      origin: off ? "group offboard" : "group role grant",
      requester: `you · group ${myRole ?? "member"}`,
      title: ceremonyTitle(a, who),
      rows: [
        { k: "Member", v: `${who} (${shortAddr(address)})` },
        ...(off ? [] : [{ k: "New role", v: roleWord(a.to) }]),
        { k: "Group", v: groupName },
        { k: "Signed with", v: "this device's comms key (not your wallet); every member checks it" },
        { k: "Effect", v: off ? "removed from messaging, roster, cluster and roles" : "their role in this group changes for everyone" },
      ],
      cost: "—",
      sponsor: "HIC-1 · you approve one change",
      sponsorColor: off ? "var(--danger)" : "var(--ok)",
      chainless: true,
      ...(off ? { warning: "Offboarding removes them across all planes in one epoch and rotates the group secret. This cannot be undone." } : {}),
      apply: () => {
        if (!stillOn(groupId)) return;
        setApplying({ address, to: a.to });
        void (off ? offboardMember(groupId, address) : assignRole(groupId, address, a.to as GroupRole)).then(() => {
          // A refusal lands in the slice's error; the row stops showing "applying".
          if (groupsSlice.get().error) setApplying(null);
          else store.toast("Signed and sent to the group. Members apply it when their devices receive it.");
        });
      },
    });
  };

  const doDirectorySearch = async () => {
    const q = dirQ.trim().replace(/^@/, "");
    if (!q) return setDirHits([]);
    try {
      setDirHits(await bridge.social.directorySearch(dirPlatform, q));
    } catch (e) {
      setDirHits([]);
      store.toast("Couldn't search the directory: " + (e instanceof Error ? e.message : String(e)));
    }
  };

  const candidates = useMemo(
    () => filterPeople(addablePeople(s.people, st.roster.map((r) => r.address)), pickerQ),
    [s.people, st.roster, pickerQ],
  );

  if (st.groups.length === 0) {
    return (
      <div className="fl-cols" data-board="members" data-state="no-groups">
        <main className="fl-main">
          <h1 className="grp-h1">Members</h1>
          <p className="grp-note">You are not in a group yet. Create one or join with an invite in Groups › Chat, then its members show here.</p>
          <span>
            <button className="btn btn-primary btn-sm" onClick={() => store.go("groups/chat")}>Go to Groups › Chat</button>
          </span>
          {st.error && <div role="alert" className="grp-alert">{st.error}</div>}
        </main>
      </div>
    );
  }

  const gName = selected ? groupLabel(st, selected) : "";
  const onlyYou = rosterRead && st.roster.length === 1 && st.roster[0].address.toLowerCase() === me;
  const state = !selected ? "no-selection" : !rosterRead ? "roster-unread" : onlyYou ? "only-you" : "roster";

  return (
    <div className="fl-cols" data-board="members" data-state={state}>
      <main className="fl-main" aria-labelledby="members-h">
        <div className="grp-head">
          <h1 id="members-h" className="grp-h1">{selected ? `${gName} members` : "Members"}</h1>
          {rosterRead && (
            <span className="fl-chip" title="Source: the group roster with signed role grants (#637)">
              {st.roster.length} {st.roster.length === 1 ? "person" : "people"}
            </span>
          )}
          <span className="grp-spacer" />
          {st.groups.length > 1 && (
            <label className="grp-inline">
              <span className="sr-only">Group</span>
              <select className="input" value={st.selectedId ?? ""} onChange={(e) => e.target.value && void selectGroup(e.target.value)}>
                {st.groups.map((g) => (
                  <option key={g.id} value={g.id}>{groupLabel(st, g)}</option>
                ))}
              </select>
            </label>
          )}
          <button className="btn btn-sm" onClick={() => store.go("groups/chat")}>Back to chat</button>
        </div>

        {st.error && <div role="alert" className="grp-alert">{st.error}</div>}

        {!rosterRead ? (
          <div role="status" className="fl-card grp-pad">
            <b>Roster not read yet</b>
            <span className="grp-muted">The comms daemon has not returned this group's roster. Roles and actions show once it does.</span>
          </div>
        ) : (
          <div className="fl-card">
            <table className="fl-tbl grp-tbl">
              <caption className="sr-only">Members of {gName}, their devices, presence, role and the actions you may take</caption>
              <thead>
                <tr><th scope="col">Member</th><th scope="col">Devices</th><th scope="col">Seen</th><th scope="col">Role</th><th scope="col">Actions</th></tr>
              </thead>
              <tbody>
                {st.roster.map((r) => {
                  const self = r.address.toLowerCase() === me;
                  const p = presenceOf(r.address, devices, devicesWhy);
                  const acts = roleActions(myRole, r.role, self);
                  const busy = st.busyMember === r.address || applying?.address === r.address;
                  return (
                    <tr key={r.address} data-member={r.address}>
                      <td>
                        <span className="grp-stack">
                          <span>{nameOf(r.address)}</span>
                          <span className="mono grp-faint">{shortAddr(r.address)}</span>
                        </span>
                      </td>
                      <td className="grp-muted" title={p.why ?? undefined}>{p.devices}{p.why && <span className="sr-only">: {p.why}</span>}</td>
                      <td className="grp-muted" title={p.why ?? undefined}>{p.seen}</td>
                      <td>
                        <span className="fl-chip" style={{ color: ROLE_TONE[r.role] ?? "var(--tx-2)" }}>{roleWord(r.role)}</span>
                        {applying?.address === r.address && (
                          <span role="status" className="fl-chip fl-chip-warn" style={{ marginLeft: 6 }}>
                            {applying.to === "removed" ? "offboarding" : `applying: ${roleWord(applying.to)}`}
                          </span>
                        )}
                      </td>
                      <td>
                        <span className="grp-row-actions">
                          {acts.actions.map((a) => (
                            <button
                              key={a.label}
                              className={"btn btn-sm" + (a.danger ? " btn-danger" : a.label === "Read-only" ? " btn-ghost" : "")}
                              disabled={busy}
                              onClick={() => selected && ask(selected.id, gName, r.address, a)}
                            >
                              {a.label}
                            </button>
                          ))}
                          {acts.none && <span className="grp-faint">{acts.none}</span>}
                        </span>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        {onlyYou && <p className="grp-note">Only you so far. Invite someone with a single-use link.</p>}
        <p className="grp-note">
          Roles: owner and admin can invite, assign roles and open rounds; member can join rounds; read-only can read. Only the owner makes or unmakes an admin.
          Each change is a signed member action every member checks.
        </p>
      </main>

      <aside className="fl-rail" aria-label="Invite people">
        <span className="eyebrow">Invite someone</span>
        {selected && manage ? (
          <>
            <InvitePanel key={selected.id} store={store} groupId={selected.id} inviterName={store.identity().name} onAdded={() => void selectGroup(selected.id)} />
            <p className="grp-note">Single use. The person redeems it in Groups › Join with an invite. They join as a member; change their role here after they join.</p>
          </>
        ) : (
          <p className="grp-note">
            Only owners and admins can invite.{managers.length ? ` Ask ${managers.join(" or ")}.` : ""}
          </p>
        )}

        {selected && manage && (
          <>
            <span className="eyebrow">Add someone you know</span>
            <label className="sr-only" htmlFor="members-people">Search people you share a group with</label>
            <input id="members-people" className="input" placeholder="Search people you share a group with" value={pickerQ} onChange={(e) => setPickerQ(e.target.value)} />
            {candidates.length === 0 ? (
              <span className="grp-note">
                {s.people.length === 0 ? "No one in your people yet. Someone appears here once you share a group with them." : pickerQ ? "No match in your people." : "Everyone you share a group with is already here."}
              </span>
            ) : (
              <ul className="grp-list" aria-label="People you can add">
                {candidates.slice(0, 20).map((p) => (
                  <li key={p.address} className="grp-list-row">
                    <span className="grp-stack" style={{ flex: 1, minWidth: 0 }}>
                      <span>{p.face ? "@" + p.face.handle : shortAddr(p.address)}</span>
                      {p.groups.length > 0 && <span className="mono grp-faint">{p.groups.map((g) => g.name).slice(0, 2).join(", ")}</span>}
                    </span>
                    <button className="btn btn-sm" onClick={() => void addMemberToGroup(selected.id, p.address)}>Add</button>
                  </li>
                ))}
              </ul>
            )}
            <details className="grp-details">
              <summary>More ways to add someone</summary>
              <div className="grp-stack" style={{ gap: 8 }}>
                <span className="grp-inline">
                  {(["x", "discord"] as const).map((pl) => (
                    <button key={pl} className={"btn btn-sm" + (dirPlatform === pl ? "" : " btn-ghost")} aria-pressed={dirPlatform === pl} onClick={() => { setDirPlatform(pl); setDirHits([]); }}>
                      {pl === "x" ? "X" : "Discord"}
                    </button>
                  ))}
                </span>
                <label className="sr-only" htmlFor="members-dir">Handle to find</label>
                <input id="members-dir" className="input" placeholder={dirPlatform === "x" ? "@handle on X" : "username on Discord"} value={dirQ} onChange={(e) => setDirQ(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void doDirectorySearch()} />
                <span><button className="btn btn-sm" onClick={() => void doDirectorySearch()}>Search</button></span>
                {dirHits.map((h) => (
                  <span key={h.address} className="grp-list-row">
                    <span style={{ flex: 1 }}>@{h.handle} <span className="mono grp-faint">{shortAddr(h.address)} · self-published</span></span>
                    <button className="btn btn-sm" onClick={() => void addMemberToGroup(selected.id, h.address)}>Add</button>
                  </span>
                ))}
                <span className="grp-note">Only people who opted in to being found by handle show here. Citrate never looks up an address from a handle otherwise.</span>
                <label className="sr-only" htmlFor="members-addr">Comms address</label>
                <input id="members-addr" className="input mono" placeholder="0x comms address" value={addr} onChange={(e) => setAddr(e.target.value)} />
                <span>
                  <button className="btn btn-sm" disabled={!addr.trim() || addr.trim().startsWith("@")} onClick={() => { void addMemberToGroup(selected.id, addr.trim()); setAddr(""); }}>Add by address</button>
                </span>
              </div>
            </details>
          </>
        )}
      </aside>
    </div>
  );
}
