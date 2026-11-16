// =====================================================================
// citrate-core — Groups › Chat (0.5.4 FL design, board 02)
//
// Three columns: your groups (create, join, search, pending invites), the selected group's thread,
// and the group's details (your role, members and presence, cluster, rounds). Alerts is the same
// board with the alerts list in the middle. The thread renders plain messages, replies in threads,
// Hermes answers with their disclosure line and what Hermes read, and round cards rendered by core.
//
// Hermes answers only when a member mentions it, on that member's own device, from the group's
// context alone (groupHermes.ts). Round cards never offer joining: today the round ledgers are
// deployed and hold no rounds, so a draft reads "Group rounds not live yet".
//
// Data sources (Rule 7):
//   groups, roster, messages   comms-member-daemon (MLS) via shell/slices/groups
//   presence                   cluster daemon `cluster_devices` / `cluster_status` (live), else "—"
//   epoch                      not reported by the comms daemon this app runs: "—" with that reason
//   Hermes                     the local model (`ai_chat_local`), group context only
//   rounds                     address book + eth_getCode through fl/groupRoundsLedger.ts
//   invites                    invites.rs (HaveAnInvite, InvitePanel)
// The relay only ever sees ciphertext; roles are checked by every member.
// =====================================================================
import { useEffect, useMemo, useRef, useState } from "react";
import type { SurfaceProps } from "../shared";
import { bridge } from "../../bridge";
import { Markdown } from "../../components/Markdown";
import type { ClusterMemberDevices, ClusterStatus, IncomingRequest, ResolvedIdentity } from "../../bridge/domains";
import { SOCIAL_BINDING_MSG_PREFIX } from "../../bridge/domains";
import { ingestDeviceLinkMessages, isDeviceLinkMessage } from "../../fleet/deviceLinkShare";
import { HaveAnInvite } from "../HaveAnInvite";
import { RelayStatusChip } from "../RelayStatusChip";
import { groupsSlice, refreshGroups, selectGroup, reloadMessages, sendMessageTo, groupLabel, mySendAddress, plainText } from "../../shell/slices/groups";
import { encodeReply, lastLine, messageRef, roleOf, threadModel, type ThreadItem } from "./groupEnvelope";
import { answerInGroup, mentionsHermes, NOT_READ, ownAnswers, rememberOwnAnswer, type GroupContext } from "./groupHermes";
import { myRoleIn, presenceOf, roleWord } from "./memberRoles";
import { liveRoundsFor, roundsLine, RoundDraftCard, RoundEventCard, ROUNDS_NOT_LIVE, useRoundsToday, type RoundsToday } from "./RoundCards";
import { groupAlerts, type GroupAlert } from "./groupAlerts";
import { CreateGroupSheet, JoinInviteSheet, KINDS, ShareFileSheet } from "./GroupSheets";
import { HicBadge } from "../../fl/primitives";
import { handOffDraft } from "./roundDraftLink";
import "./groups.css";

const shortAddr = (a: string): string => (!a ? "—" : a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a);
const initials = (s: string): string => (s.replace(/^0x/, "").slice(0, 2) || "··").toUpperCase();

/** Why the epoch shows "—". */
export const EPOCH_UNKNOWN = "The comms daemon this app runs does not report the MLS epoch yet.";
/** The line under the composer. */
export const HERMES_DISCLOSURE = "@Hermes runs on your device; its reply is posted to everyone in this group.";

type HermesRun = { kind: "idle" } | { kind: "running"; groupId: string } | { kind: "failed"; groupId: string; reason: string };

export interface GroupChatProps extends SurfaceProps {
  view?: "groups" | "alerts";
  /** Defaults to the shipped address book reading; tests pass a fixed reading. */
  rounds?: RoundsToday;
}

export function GroupChat(props: GroupChatProps) {
  const live = useRoundsToday();
  return <GroupChatBoard {...props} rounds={props.rounds ?? live} />;
}

function GroupChatBoard({ store, s, view = "groups", rounds }: GroupChatProps & { rounds: RoundsToday }) {
  const st = groupsSlice.use();
  const [query, setQuery] = useState("");
  const [sheet, setSheet] = useState<"" | "create" | "join" | "share">("");
  const [draft, setDraft] = useState("");
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const [replyTo, setReplyTo] = useState<string | null>(null);
  const [replyText, setReplyText] = useState("");
  const [hermes, setHermes] = useState<HermesRun>({ kind: "idle" });
  const [own, setOwn] = useState<Set<string>>(() => ownAnswers());
  const [faces, setFaces] = useState<Record<string, ResolvedIdentity>>({});
  const [devices, setDevices] = useState<ClusterMemberDevices[] | null>(null);
  const [devicesWhy, setDevicesWhy] = useState("Reading the group's cluster.");
  const [cluster, setCluster] = useState<ClusterStatus | null>(null);
  const [requests, setRequests] = useState<Record<string, IncomingRequest[]>>({});
  const composer = useRef<HTMLTextAreaElement>(null);

  const selected = st.groups.find((g) => g.id === st.selectedId) ?? null;
  const myWallet = typeof store.identity === "function" ? store.identity().wallet : "";
  const me = (s.commsAddr || myWallet || s.walletAddr || "").toLowerCase();
  const navRole = selected ? s.myGroups.find((g) => g.id === selected.id)?.myRole : null;
  const myRole = selected ? myRoleIn(navRole, st.roster, selected.owner, me) : null;
  const readOnly = myRole === "guest";

  useEffect(() => {
    void refreshGroups();
    void store.refreshPeople();
  }, []);

  // Open on a group: the selected one, else the first you are in.
  useEffect(() => {
    if (!st.selectedId && st.groups.length > 0) void selectGroup(st.groups[0].id);
  }, [st.selectedId, st.groups.length]);

  // While a group is open, drain its mailbox so other members' messages arrive. An unreachable relay
  // is an honest no-op, never a fabricated message.
  useEffect(() => {
    if (!selected) return;
    const id = selected.id;
    const t = setInterval(() => void reloadMessages(id), 4000);
    return () => clearInterval(t);
  }, [selected?.id]);

  // Presence and the peer count come from the group's cluster. A failed read is "—" with its reason.
  useEffect(() => {
    if (!selected) return;
    let on = true;
    setDevices(null);
    setCluster(null);
    setDevicesWhy("Reading the group's cluster.");
    bridge.cluster
      .devices(selected.id)
      .then((d) => on && setDevices(d))
      .catch((e) => on && setDevicesWhy("The group's cluster did not answer: " + (e instanceof Error ? e.message : String(e))));
    bridge.cluster
      .status(selected.id)
      .then((c) => on && setCluster(c))
      .catch(() => {
        /* the rail shows "—" */
      });
    return () => {
      on = false;
    };
  }, [selected?.id]);

  // Verified faces for the addresses on screen; the short address otherwise.
  useEffect(() => {
    const addrs = Array.from(new Set([...st.roster.map((r) => r.address), ...st.messages.map((m) => m.sender)])).filter(Boolean);
    if (!addrs.length) return;
    let on = true;
    bridge.social
      .resolve(addrs)
      .then((res) => {
        if (!on) return;
        const m: Record<string, ResolvedIdentity> = {};
        for (const r of res) m[r.address.toLowerCase()] = r;
        setFaces(m);
      })
      .catch(() => {
        /* no faces resolved */
      });
    return () => {
      on = false;
    };
  }, [st.roster, st.messages.length]);

  // Control messages: verified social bindings (D1) and DeviceLink shares (HUP-S8.1). Never shown.
  useEffect(() => {
    const ctrl = st.messages.filter((m) => m.body.startsWith(SOCIAL_BINDING_MSG_PREFIX));
    for (const m of ctrl) {
      try {
        void bridge.social.ingestBinding(m.sender, JSON.parse(m.body.slice(SOCIAL_BINDING_MSG_PREFIX.length))).catch(() => {});
      } catch {
        /* a malformed control message is ignored */
      }
    }
    const shares = st.messages.filter((m) => isDeviceLinkMessage(m.body));
    if (shares.length) void ingestDeviceLinkMessages(shares, (sender, body) => bridge.cluster.deviceLinksIngest(sender, body));
  }, [st.messages]);

  // Alerts: waiting invite requests for the groups you manage (the relay's claims inbox).
  useEffect(() => {
    if (view !== "alerts") return;
    let on = true;
    const managed = s.myGroups.filter((g) => g.iManage);
    void Promise.all(
      managed.map((g) =>
        bridge.invites
          .requests(g.id)
          .then((r) => [g.id, r] as const)
          .catch(() => [g.id, [] as IncomingRequest[]] as const),
      ),
    ).then((pairs) => on && setRequests(Object.fromEntries(pairs)));
    return () => {
      on = false;
    };
  }, [view, s.myGroups]);

  const nameOf = (a: string): string => {
    if (a.toLowerCase() === me || a === "you") return "You";
    const f = faces[a.toLowerCase()];
    return f ? "@" + f.handle : shortAddr(a);
  };
  const thread = useMemo(() => threadModel(st.messages), [st.messages]);
  const gName = selected ? groupLabel(st, selected) : "";
  const kindLabel = selected ? KINDS.find((k) => k.id === selected.kind)?.label ?? selected.kind : "";
  const shownGroups = st.groups.filter((g) => !query.trim() || groupLabel(st, g).toLowerCase().includes(query.trim().toLowerCase()));
  const onlineCount = devices ? st.roster.filter((r) => presenceOf(r.address, devices, "").seen === "online").length : null;
  const live = selected ? liveRoundsFor(selected.id) : [];
  const drafts = thread.filter((t): t is Extract<ThreadItem, { kind: "hermes" }> => t.kind === "hermes" && !!t.draft);

  const send = async () => {
    if (!selected || readOnly) return;
    // What is sent is the typed text with control characters removed (the slice strips them too).
    const text = plainText(draft).trim();
    if (!text) return;
    // Everything Hermes reads is taken now, for the group this message goes to: the member may
    // switch groups while the send and the model run, and the answer must stay in this group.
    const groupId = selected.id;
    const at = {
      groupName: gName,
      roster: st.roster,
      cluster: cluster ? { online: cluster.online, total: cluster.total } : null,
      rounds: roundsLine(rounds),
    };
    setDraft("");
    try {
      await sendMessageTo(groupId, text);
    } catch {
      return;
    }
    if (!mentionsHermes(text)) return;
    const asker = await mySendAddress();
    const c: GroupContext = { ...at, messages: groupsSlice.get().history[groupId] ?? [], asker };
    setHermes({ kind: "running", groupId });
    const out = await answerInGroup(groupId, messageRef(asker, text), c, {
      inferLocal: (m, ctx) => bridge.chat.inferLocal(m, ctx),
      send: async (gid, body) => {
        rememberOwnAnswer(messageRef(asker, body));
        setOwn(ownAnswers());
        await sendMessageTo(gid, body, { structured: true });
      },
    });
    if (out.kind === "posted") setHermes({ kind: "idle" });
    else if (out.kind === "no-answer") setHermes({ kind: "failed", groupId, reason: `Hermes did not answer on this device (${out.reason}). Nothing was posted to the group.` });
    else setHermes({ kind: "failed", groupId, reason: `Hermes answered, but the answer was not sent to the group (${out.reason}).` });
  };

  const sendReply = async (to: string) => {
    if (!selected || readOnly) return;
    const text = replyText.trim();
    if (!text) return;
    setReplyText("");
    setReplyTo(null);
    setOpen((o) => ({ ...o, [to]: true }));
    await sendMessageTo(selected.id, encodeReply(to, text), { structured: true }).catch(() => {});
  };

  const openAlert = (a: GroupAlert) => {
    void selectGroup(a.groupId);
    store.go(a.route);
  };

  const alerts =
    view === "alerts"
      ? groupAlerts({
          groups: st.groups.map((g) => ({ id: g.id, name: groupLabel(st, g) })),
          history: st.history,
          roles: Object.fromEntries(s.myGroups.map((g) => [g.id, g.myRole])),
          requests,
          me,
          ownAnswers: own,
        })
      : [];

  const roleChip = (addr: string) => {
    const r = roleOf(st.roster, addr);
    return r ? <span className="fl-chip">{roleWord(r)}</span> : null;
  };

  const renderReplies = (item: ThreadItem) => {
    const n = item.replies.length;
    const shown = open[item.ref];
    return (
      <>
        <span className="grp-inline">
          {n > 0 && (
            <button className="btn btn-ghost btn-sm" aria-expanded={shown ? "true" : "false"} onClick={() => setOpen((o) => ({ ...o, [item.ref]: !shown }))}>
              {shown ? "Hide replies" : `${n} ${n === 1 ? "reply" : "replies"}`}
            </button>
          )}
          {!readOnly && (
            <button className="btn btn-ghost btn-sm" onClick={() => { setReplyTo(replyTo === item.ref ? null : item.ref); setReplyText(""); }}>
              Reply
            </button>
          )}
        </span>
        {shown && n > 0 && (
          <div className="grp-replies" role="list" aria-label="Replies">
            {item.replies.map((r) => (
              <span key={r.ref} role="listitem" style={{ fontSize: 12.5 }}>
                <b style={{ fontWeight: 500 }}>{nameOf(r.sender)}</b> {r.text}
              </span>
            ))}
          </div>
        )}
        {replyTo === item.ref && (
          <span className="grp-inline" style={{ marginTop: 4 }}>
            <label className="sr-only" htmlFor={"reply-" + item.ref}>Reply in thread</label>
            <input id={"reply-" + item.ref} className="input" style={{ flex: 1, minWidth: 180 }} placeholder="Reply in thread" value={replyText} onChange={(e) => setReplyText(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void sendReply(item.ref)} />
            <button className="btn btn-sm btn-primary" disabled={!replyText.trim()} onClick={() => void sendReply(item.ref)}>Send reply</button>
          </span>
        )}
      </>
    );
  };

  const renderItem = (item: ThreadItem) => {
    if (item.kind === "hermes") {
      const asker = nameOf(item.device);
      // Verified only for an answer this device posted itself; anything else is a message that
      // claims to be a Hermes answer, attributed to the member whose device sent it.
      const verified = asker === "You" && own.has(item.ref);
      return (
        <article key={item.ref} className="grp-msg" aria-label={verified ? "Hermes reply in group" : "Message posted as a Hermes answer, not verified"} data-testid="group-hermes-reply" data-verified={verified ? "yes" : "no"}>
          <span className="grp-avatar grp-avatar-agent" aria-hidden="true">H</span>
          <div className="grp-msg-body">
            <div className="grp-msg-meta">
              <b style={{ fontWeight: 500 }}>Hermes</b>
              {verified ? (
                <span className="grp-faint">on your device · answered because you mentioned it · visible to everyone here</span>
              ) : (
                <span className="grp-faint">posted from {asker === "You" ? "your" : `${asker}’s`} device as a Hermes answer (not verified) · visible to everyone here</span>
              )}
            </div>
            <Markdown text={item.text} />
            <details className="grp-details">
              <summary>{verified ? "What Hermes read" : "What the message says Hermes read"} ({item.read.length})</summary>
              <ul className="grp-note" style={{ margin: "4px 0", paddingLeft: 18 }}>
                {item.read.map((r) => <li key={r}>{r}</li>)}
              </ul>
              <p className="grp-note">{NOT_READ}</p>
            </details>
            {item.draft && (
              <RoundDraftCard
                draft={item.draft}
                rounds={rounds}
                myRole={myRole}
                verified={verified}
                onReview={() => {
                  const d = item.draft!;
                  const gid = selected!.id;
                  void handOffDraft(d, gid, gName).then((handed) => {
                    if (!handed) store.toast("Open a round could not take the draft in this build; enter its values there.");
                    store.go("groups/rounds/open");
                  });
                }}
              />
            )}
            {renderReplies(item)}
          </div>
        </article>
      );
    }
    const isAgent = roleOf(st.roster, item.sender) === "agent";
    return (
      <div key={item.ref} className="grp-msg" data-testid="group-message">
        <span className={"grp-avatar" + (isAgent ? " grp-avatar-agent" : "")} aria-hidden="true">{initials(nameOf(item.sender).replace(/^@/, ""))}</span>
        <div className="grp-msg-body">
          <div className="grp-msg-meta">
            <b style={{ fontWeight: 500 }}>{nameOf(item.sender)}</b>
            {roleChip(item.sender)}
          </div>
          <Markdown text={item.text} />
          {renderReplies(item)}
        </div>
      </div>
    );
  };

  const noGroups = st.groups.length === 0;
  const state = view === "alerts" ? "alerts" : noGroups ? "empty" : selected ? "thread" : "loading";

  return (
    <div className="grp-page grp-cols-3" data-board="group-chat" data-state={state}>
      <aside className="grp-col grp-list-col" aria-label="Your groups">
        <span className="grp-inline">
          <button className="btn btn-primary btn-sm" onClick={() => setSheet("create")}>Create a group</button>
          <button className="btn btn-sm" onClick={() => setSheet("join")}>Join with an invite</button>
        </span>
        <RelayStatusChip />
        <label className="sr-only" htmlFor="grp-search">Search groups</label>
        <input id="grp-search" className="input" placeholder="Search groups" value={query} onChange={(e) => setQuery(e.target.value)} />
        <HaveAnInvite store={store} s={s} memberOf={st.groups.map((g) => g.id)} pasteField={false} />
        <nav aria-label="Groups">
          {shownGroups.map((g) => {
            const name = groupLabel(st, g);
            const cur = g.id === st.selectedId && view === "groups";
            const last = lastLine(st.history[g.id] ?? []);
            return (
              <button
                key={g.id}
                className="grp-group-btn"
                aria-current={cur ? "page" : undefined}
                onClick={() => {
                  void selectGroup(g.id);
                  if (view !== "groups") store.go("groups/chat");
                }}
              >
                <span className="grp-avatar" aria-hidden="true">{initials(name)}</span>
                <span className="grp-stack" style={{ flex: 1 }}>
                  <span style={{ fontSize: 13, fontWeight: 500, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{name}</span>
                  {last && <span className="grp-faint" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{last}</span>}
                </span>
              </button>
            );
          })}
        </nav>
        {noGroups && <p className="grp-note">Groups you create or join show here. Reach stays in your groups.</p>}
        {st.error && <div role="alert" className="grp-alert">{st.error}</div>}
      </aside>

      {view === "alerts" ? (
        <main className="grp-col" aria-labelledby="alerts-h">
          <h1 id="alerts-h" className="grp-h1">Alerts</h1>
          {alerts.length === 0 ? (
            <p className="grp-note" data-testid="alerts-empty">Nothing needs you right now. Round drafts waiting for review, Hermes answers to your messages and invite requests show here.</p>
          ) : (
            <ul className="grp-list" aria-label="Alerts">
              {alerts.map((a) => (
                <li key={a.key}>
                  <button className="fl-card grp-pad" style={{ width: "100%", textAlign: "left", cursor: "pointer", color: "var(--tx-1)", flexDirection: "row", alignItems: "center", gap: 10 }} onClick={() => openAlert(a)} data-alert={a.kind}>
                    {a.tag === "HIC-1" ? <HicBadge level="HIC-1" /> : <span className="fl-chip">{a.tag}</span>}
                    <span style={{ flex: 1, fontSize: 13 }}>{a.text}</span>
                    <span className="grp-faint">{a.route === "groups/members" ? "Members" : "Chat"}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          <p className="grp-note">An alert opens the screen where you act. Notifications point here. They never carry an approve button.</p>
        </main>
      ) : noGroups ? (
        <main className="grp-col lattice-dots" aria-labelledby="empty-h" style={{ justifyContent: "center", alignItems: "flex-start", gridColumn: "2 / -1" }}>
          <h1 id="empty-h" className="grp-h1" style={{ fontSize: 30 }}>No groups yet</h1>
          <p className="grp-muted" style={{ maxWidth: 520, fontSize: 14, lineHeight: 1.6, margin: 0 }}>
            A group is an encrypted space for the people you train with. Groups link machines into a cluster and open rounds together. Roles are checked by every member, and the relay only ever sees ciphertext.
          </p>
          <span className="grp-inline">
            <button className="btn btn-primary btn-lg" onClick={() => setSheet("create")}>Create a group</button>
            <button className="btn btn-lg" onClick={() => setSheet("join")}>Join with an invite</button>
          </span>
        </main>
      ) : !selected ? (
        <main className="grp-col" aria-busy="true" style={{ gridColumn: "2 / -1" }}>
          <p className="grp-note">Opening your group.</p>
        </main>
      ) : (
        <>
          <main className="grp-thread" aria-labelledby="thread-h">
            <div className="grp-thread-head">
              <h1 id="thread-h" className="grp-h1" style={{ fontSize: 20 }}>{gName}</h1>
              <span className="fl-chip">{kindLabel}</span>
              <span className="fl-chip" title={EPOCH_UNKNOWN} data-testid="group-epoch">
                Encrypted · epoch —<span className="sr-only"> ({EPOCH_UNKNOWN})</span>
              </span>
              <span className="grp-spacer" />
              <button className="btn btn-ghost btn-sm" onClick={() => store.go("groups/members")} title={devices ? "Presence from the group's cluster" : devicesWhy}>
                {st.roster.length} {st.roster.length === 1 ? "member" : "members"} · {onlineCount === null ? "online —" : `${onlineCount} online`}
              </button>
            </div>
            <div className="grp-thread-scroll" aria-live="polite" aria-relevant="additions" data-testid="group-thread">
              {thread.length === 0 ? (
                <p className="grp-note" style={{ margin: "auto", textAlign: "center", maxWidth: 360 }}>
                  Nothing here yet. Messages reach every member of this group, and nobody outside it.
                </p>
              ) : (
                thread.map(renderItem)
              )}
              {live.map((r) => (
                <RoundEventCard key={r.round} r={r} onOpen={() => store.go("groups/rounds/live")} />
              ))}
              {hermes.kind === "running" && hermes.groupId === selected.id && <div role="status" className="grp-typing">Hermes is answering on your device.</div>}
              {hermes.kind === "failed" && hermes.groupId === selected.id && (
                <div role="alert" className="grp-alert">
                  {hermes.reason}{" "}
                  <button className="btn btn-ghost btn-sm" onClick={() => setHermes({ kind: "idle" })}>Dismiss</button>
                </div>
              )}
            </div>
            <div className="grp-composer">
              <div className="grp-inline" style={{ flexWrap: "nowrap", alignItems: "flex-end" }}>
                <button className="btn btn-ghost btn-sm" aria-label="Share a file" title="Share a file" disabled={readOnly} onClick={() => setSheet("share")}>File</button>
                <label className="sr-only" htmlFor="grp-msg">Message {gName}</label>
                <textarea
                  id="grp-msg"
                  ref={composer}
                  className="input"
                  rows={1}
                  style={{ flex: 1, minWidth: 0 }}
                  placeholder={readOnly ? "Read-only members cannot post here" : `Message ${gName}`}
                  disabled={readOnly}
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      void send();
                    }
                  }}
                />
                <button
                  className="btn btn-ghost btn-sm"
                  disabled={readOnly}
                  onClick={() => {
                    if (!mentionsHermes(draft)) setDraft((d) => (d ? d.replace(/\s*$/, " ") : "") + "@Hermes ");
                    composer.current?.focus();
                  }}
                >
                  @Hermes
                </button>
                <button className="btn btn-primary btn-sm" disabled={readOnly || st.sending || !draft.trim()} onClick={() => void send()}>Send</button>
              </div>
              <span className="grp-faint">{HERMES_DISCLOSURE}</span>
            </div>
          </main>

          <aside className="grp-col grp-rail" aria-label="Group details">
            <section className="grp-stack" style={{ gap: 6 }}>
              <span className="eyebrow">{gName}</span>
              <span className="grp-inline">
                <span className="grp-muted" style={{ fontSize: 12 }}>Your role</span>
                <span className="fl-chip fl-chip-ok">{myRole ? roleWord(myRole) : "—"}</span>
              </span>
            </section>
            <section className="grp-stack" style={{ gap: 6 }}>
              <span className="grp-inline">
                <span className="eyebrow">Members</span>
                <span className="grp-spacer" />
                <button className="btn btn-sm" onClick={() => store.go("groups/members")}>{myRole === "owner" || myRole === "admin" ? "Assign roles" : "View all"}</button>
              </span>
              <ul className="grp-list" aria-label="Members and presence">
                {st.roster.slice(0, 12).map((r) => {
                  const p = presenceOf(r.address, devices, devicesWhy);
                  return (
                    <li key={r.address} className="grp-list-row" style={{ fontSize: 12.5 }}>
                      <span aria-hidden="true" style={{ width: 7, height: 7, borderRadius: 999, background: p.seen === "online" ? "var(--ok)" : "var(--line-2)", flexShrink: 0 }} />
                      <span style={{ flex: 1, minWidth: 0 }}>{nameOf(r.address)}</span>
                      <span className="grp-faint" title={p.why ?? undefined}>{p.seen}</span>
                      <span className="fl-chip">{roleWord(r.role)}</span>
                    </li>
                  );
                })}
              </ul>
            </section>
            <section className="grp-stack" style={{ gap: 6 }}>
              <span className="eyebrow">Cluster</span>
              <div className="fl-card-2 grp-stats">
                <div>
                  <div className="grp-stat-v">{cluster ? `${cluster.online}/${cluster.total}` : "—"}</div>
                  <div className="grp-faint">{cluster ? "peers online / authorized" : "the cluster did not answer"}</div>
                </div>
                <div>
                  <div className="grp-stat-v">{cluster ? cluster.sharedFiles.length : "—"}</div>
                  <div className="grp-faint">shared files</div>
                </div>
              </div>
              <span>
                <button className="btn btn-ghost btn-sm" onClick={() => store.go("groups/cluster")}>Open the cluster map</button>
              </span>
            </section>
            <section className="grp-stack" style={{ gap: 6 }} data-testid="rail-rounds">
              <span className="eyebrow">Rounds</span>
              {live.map((r) => (
                <button key={r.round} className="fl-card-2 grp-pad" onClick={() => store.go("groups/rounds/live")} style={{ textAlign: "left", cursor: "pointer" }}>
                  Round {r.round} · {r.stage}
                </button>
              ))}
              {drafts.map((d) => (
                <span key={d.ref} className="fl-card-2 grp-pad" style={{ fontSize: 12.5 }}>
                  {d.draft!.title} <span className="grp-faint">Draft</span>
                </span>
              ))}
              {rounds.kind === "not-live" && <span className="grp-note">{ROUNDS_NOT_LIVE}. {rounds.detail}</span>}
              {rounds.kind === "unknown" && <span className="grp-note">Rounds: — The round ledger read failed ({rounds.reason}).</span>}
              {rounds.kind === "reading" && <span className="grp-note">Reading the round ledgers.</span>}
              {rounds.kind === "live" && live.length === 0 && <span className="grp-note">No round in this group right now.</span>}
            </section>
          </aside>
        </>
      )}

      {sheet === "create" && <CreateGroupSheet store={store} onClose={() => setSheet("")} />}
      {sheet === "join" && <JoinInviteSheet store={store} memberOf={st.groups.map((g) => g.id)} onClose={() => setSheet("")} />}
      {sheet === "share" && selected && <ShareFileSheet store={store} groupId={selected.id} onClose={() => setSheet("")} />}
    </div>
  );
}

