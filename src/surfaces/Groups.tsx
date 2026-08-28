// =====================================================================
// citrate-core — Groups (CX-S3.4, lane s3)
//
// Secure 1:1 + group chat over the comms-member-daemon seam (bridge.groups). A group list, a
// conversation pane, a composer, and an owner/admin roster with roles + atomic offboard. RBAC is
// enforced at the RELAY, not the client (planset RT / ADR-001) — so the surface offers the controls
// and shows the relay's honest error if you are not authorized; it never fakes success.
//
// Honest states throughout (Rule 1): empty when there is nothing, real error text on failure (incl.
// "comms identity not provisioned" / "binary not bundled" before the daemon is packaged). No room,
// roster, or message is ever fabricated. Reach is scoped to your Groups (RT-6).
// =====================================================================
import { useEffect, useState } from "react";
import { SurfaceProps } from "./shared";
import type { Group, GroupRole } from "../bridge/domains";
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
  groupLabel,
} from "../shell/slices/groups";

const KINDS: Group["kind"][] = ["dm", "channel", "forum"];
const ROLES: GroupRole[] = ["owner", "admin", "member", "guest", "agent"];

function shortAddr(a: string): string {
  if (!a) return "—";
  return a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a;
}

export function Groups({ store }: SurfaceProps) {
  const st = groupsSlice.use();
  const [name, setName] = useState("");
  const [kind, setKind] = useState<Group["kind"]>("channel");
  const [draft, setDraft] = useState("");
  const [invite, setInvite] = useState("");

  useEffect(() => {
    void refreshGroups();
  }, []);

  const selected = st.groups.find((g) => g.id === st.selectedId) ?? null;

  const doCreate = () => {
    const n = name.trim();
    if (!n) return;
    setName("");
    void createGroup(kind, n);
  };

  const doSend = () => {
    const b = draft.trim();
    if (!b) return;
    setDraft("");
    void sendMessage(b);
  };

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, height: "100%", boxSizing: "border-box" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Groups</span>
        <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--tx-3)" }}>
          end-to-end encrypted, in your Groups
        </span>
      </div>

      <p style={{ fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.55, margin: 0, maxWidth: 640 }}>
        Message people in your Groups, 1:1 or together. A Group you own is administered with roles —
        assign and remove members — enforced at the relay, which only ever sees ciphertext.
      </p>

      {st.error && (
        <div className="surface" role="alert" style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--bad, #c0392b)", lineHeight: 1.5 }}>
          {st.error}
        </div>
      )}

      <div style={{ display: "flex", gap: 16, flex: 1, minHeight: 0 }}>
        {/* ---- left: create + group list ---- */}
        <div style={{ width: 232, display: "flex", flexDirection: "column", gap: 10, flexShrink: 0 }}>
          <div className="surface" style={{ padding: "12px", display: "flex", flexDirection: "column", gap: 8 }}>
            <input
              className="input"
              placeholder="New group name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && doCreate()}
              style={{ fontSize: 12.5 }}
            />
            <div style={{ display: "flex", gap: 8 }}>
              <select value={kind} onChange={(e) => setKind(e.target.value as Group["kind"])} className="input" style={{ fontSize: 12, flex: 1 }}>
                {KINDS.map((k) => (
                  <option key={k} value={k}>
                    {k}
                  </option>
                ))}
              </select>
              <button className="btn btn-sm" onClick={doCreate} disabled={st.creating || !name.trim()}>
                {st.creating ? "…" : "New"}
              </button>
            </div>
          </div>

          <div className="surface" style={{ flex: 1, minHeight: 0, overflowY: "auto", display: "flex", flexDirection: "column" }}>
            {st.groups.length === 0 ? (
              <div style={{ padding: "16px", fontSize: 12, color: "var(--tx-3)", lineHeight: 1.6 }}>
                No groups yet. Create one above to start a conversation.
              </div>
            ) : (
              st.groups.map((g) => {
                const active = g.id === st.selectedId;
                return (
                  <button
                    key={g.id}
                    onClick={() => void selectGroup(g.id)}
                    style={{
                      textAlign: "left",
                      padding: "10px 14px",
                      border: "none",
                      borderTop: "1px solid var(--ln, rgba(0,0,0,0.06))",
                      background: active ? "var(--bg-2, rgba(0,0,0,0.05))" : "transparent",
                      cursor: "pointer",
                      display: "flex",
                      flexDirection: "column",
                      gap: 2,
                    }}
                  >
                    <span style={{ fontSize: 12.5, fontWeight: active ? 560 : 460, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                      {groupLabel(st, g)}
                    </span>
                    <span style={{ fontSize: 10, color: "var(--tx-3)" }}>
                      {g.kind}
                      {g.members.length ? ` · ${g.members.length} member${g.members.length === 1 ? "" : "s"}` : ""}
                    </span>
                  </button>
                );
              })
            )}
          </div>
        </div>

        {/* ---- right: conversation + roster ---- */}
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 10 }}>
          {!selected ? (
            <div className="surface" style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 12.5, color: "var(--tx-3)", padding: 24, textAlign: "center" }}>
              Select a group to open the conversation, or create one.
            </div>
          ) : (
            <>
              <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                <span style={{ fontSize: 15, fontWeight: 540 }}>{groupLabel(st, selected)}</span>
                <span style={{ fontSize: 10.5, color: "var(--tx-3)" }}>{selected.kind}</span>
                <button className="btn btn-ghost btn-sm" style={{ marginLeft: "auto" }} onClick={() => void reloadMessages(selected.id)}>
                  Refresh
                </button>
              </div>

              {/* messages */}
              <div className="surface" style={{ flex: 1, minHeight: 120, overflowY: "auto", display: "flex", flexDirection: "column", gap: 8, padding: "12px 14px" }}>
                {st.messages.length === 0 ? (
                  <div style={{ margin: "auto", fontSize: 12, color: "var(--tx-3)" }}>No messages yet. Say something.</div>
                ) : (
                  st.messages.map((m) => (
                    <div key={m.id} style={{ display: "flex", flexDirection: "column", gap: 1 }}>
                      <span className="mono" style={{ fontSize: 9.5, color: "var(--tx-3)" }}>{shortAddr(m.sender)}</span>
                      <span style={{ fontSize: 13, lineHeight: 1.5, whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{m.body}</span>
                    </div>
                  ))
                )}
              </div>

              {/* composer */}
              <div style={{ display: "flex", gap: 8 }}>
                <input
                  className="input"
                  placeholder="Write a message…"
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && doSend()}
                  style={{ flex: 1, fontSize: 13 }}
                />
                <button className="btn btn-sm" onClick={doSend} disabled={st.sending || !draft.trim()}>
                  {st.sending ? "…" : "Send"}
                </button>
              </div>

              {/* roster / admin */}
              <div className="surface" style={{ padding: "10px 14px", display: "flex", flexDirection: "column", gap: 6 }}>
                <span style={{ fontSize: 10.5, color: "var(--tx-3)", textTransform: "uppercase", letterSpacing: 0.4 }}>
                  Members · roles enforced at the relay
                </span>
                {/* invite: the member must have published a key package to the relay first */}
                <div style={{ display: "flex", gap: 8 }}>
                  <input
                    className="input"
                    placeholder="Invite by address (0x…)"
                    value={invite}
                    onChange={(e) => setInvite(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && invite.trim()) {
                        void addMemberToGroup(invite.trim());
                        setInvite("");
                      }
                    }}
                    style={{ flex: 1, fontSize: 11.5 }}
                  />
                  <button
                    className="btn btn-sm"
                    disabled={!invite.trim() || st.busyMember === invite.trim()}
                    onClick={() => {
                      void addMemberToGroup(invite.trim());
                      setInvite("");
                    }}
                  >
                    Invite
                  </button>
                </div>
                {st.roster.length === 0 ? (
                  <span style={{ fontSize: 11.5, color: "var(--tx-3)" }}>Just you so far.</span>
                ) : (
                  st.roster.map((mem) => {
                    const busy = st.busyMember === mem.address;
                    return (
                      <div key={mem.address} style={{ display: "flex", alignItems: "center", gap: 10 }}>
                        <span className="mono" style={{ fontSize: 11.5, flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}>
                          {shortAddr(mem.address)}
                        </span>
                        <select
                          value={mem.role}
                          disabled={busy || mem.role === "owner"}
                          onChange={(e) => void assignRole(mem.address, e.target.value as GroupRole)}
                          className="input"
                          style={{ fontSize: 11, padding: "2px 6px" }}
                        >
                          {ROLES.map((r) => (
                            <option key={r} value={r}>
                              {r}
                            </option>
                          ))}
                        </select>
                        {mem.role !== "owner" && (
                          <button
                            className="btn btn-ghost btn-sm"
                            disabled={busy}
                            onClick={() => {
                              void offboardMember(mem.address);
                              store.toast("Removing member — the group secret rotates");
                            }}
                          >
                            {busy ? "…" : "Remove"}
                          </button>
                        )}
                      </div>
                    );
                  })
                )}
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
