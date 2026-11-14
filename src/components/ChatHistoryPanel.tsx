// 0.5.4 — prior conversations and projects, reachable from the Hermes chat (owner request
// 2026-11-14): list, search, reopen.
//
// Data sources (Rule 7):
//   Conversations — the chat history slice (src/shell/slices/chatHistory.ts): the threads of
//     AppState.chatMsgs this device has had, sealed by core (hermes_history_* commands:
//     ML-KEM-768 + AES-256-GCM under a device-sealed key in the OS keyring) and decrypted into
//     memory only.
//   Projects — derived from the same threads: each conversation holding an accepted "Plan it
//     first" brief (ChatMsg.brief, written by store.acceptBrief from the sidecar's brief_create).
//   Export — the journal's encrypted export (journal_export_encrypted), one page per conversation.
// Nothing here is fetched from a server and nothing is invented: an empty history says so.
import { useState } from "react";
import { chatHistory, projectsFrom, searchProjects, searchThreads, setHistoryRetention } from "../shell/slices/chatHistory";
import { desktopJournalIo, exportJournalEncrypted } from "../journal/encryptedExport";
import { threadsAsJournalPages } from "./historyExport";
import type { Store } from "../shell/store";

/** The retention choices offered (days; null = until deleted). */
const RETENTION_CHOICES: { label: string; days: number | null }[] = [
  { label: "Until I delete them", days: null },
  { label: "30 days", days: 30 },
  { label: "90 days", days: 90 },
  { label: "1 year", days: 365 },
];

const when = (ts: number, now: number = Date.now()) => {
  const m = (now - ts) / 60000;
  if (m < 1) return "just now";
  if (m < 60) return (m | 0) + "m ago";
  const h = m / 60;
  if (h < 24) return (h | 0) + "h ago";
  const d = h / 24;
  if (d < 7) return (d | 0) + "d ago";
  return new Date(ts).toLocaleDateString();
};

type Tab = "chats" | "projects";

export function ChatHistoryPanel({ store, currentId, busy, onOpened }: { store: Store; currentId: string; busy: boolean; onOpened: () => void }) {
  const { threads, persisted, loaded, unreadable, retentionDays, retentionUnreadable, error } = chatHistory.use();
  const [tab, setTab] = useState<Tab>("chats");
  const [query, setQuery] = useState("");
  const [manage, setManage] = useState(false);
  const [confirmErase, setConfirmErase] = useState(false);
  const [pass, setPass] = useState("");
  const [pass2, setPass2] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const exportAll = async () => {
    setNote(null);
    const r = await exportJournalEncrypted(await desktopJournalIo(), threadsAsJournalPages(threads), pass, pass2, new Date());
    if (r.ok) {
      setPass("");
      setPass2("");
      setNote(`Exported ${threads.length} conversations, encrypted, to ${r.path}. Open it from Journal › Import.`);
    } else if (!r.cancelled) setNote(r.reason);
  };
  const eraseAll = async () => {
    setConfirmErase(false);
    if (await store.forgetAllChats()) {
      setNote("Every conversation and the history key were erased.");
      onOpened();
    }
  };
  const chats = searchThreads(threads, query);
  const projects = searchProjects(projectsFrom(threads), query);
  const open = (id: string) => {
    if (store.openChat(id)) onOpened();
  };

  const tabBtn = (id: Tab, label: string, count: number) => (
    <button
      type="button"
      role="tab"
      id={"chat-history-tab-" + id}
      aria-selected={tab === id}
      aria-controls="chat-history-list"
      data-testid={"chat-history-tab-" + id}
      onClick={() => setTab(id)}
      className="mono"
      style={{ fontSize: 10, letterSpacing: ".08em", textTransform: "uppercase", padding: "4px 10px", borderRadius: 999, cursor: "pointer", border: "1px solid " + (tab === id ? "var(--tx-1)" : "var(--line-1)"), background: tab === id ? "var(--srf-2)" : "transparent", color: tab === id ? "var(--tx-1)" : "var(--tx-3)" }}
    >
      {label} · {count}
    </button>
  );

  const rowStyle = { display: "flex", alignItems: "center", gap: 10, width: "100%", textAlign: "left" as const, padding: "8px 10px", borderRadius: "var(--r-1)", border: "1px solid transparent", background: "transparent", color: "var(--tx-1)", cursor: busy ? "not-allowed" : "pointer", fontFamily: "var(--font-sans)" };

  return (
    <div data-testid="chat-history" id="chat-history" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <input
          className="input"
          type="search"
          data-testid="chat-history-search"
          aria-label="Search conversations and projects"
          placeholder="Search conversations and projects…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          style={{ flex: 1, minWidth: 0, height: 30, fontSize: 12.5 }}
        />
      </div>
      <div role="tablist" aria-label="History" style={{ display: "flex", gap: 6, alignItems: "center" }}>
        {tabBtn("chats", "Conversations", chats.length)}
        {tabBtn("projects", "Projects", projects.length)}
        <span style={{ flex: 1 }}></span>
        <span
          className="mono"
          data-testid="chat-history-where"
          title={persisted ? "Sealed on this device (ML-KEM-768 + AES-256-GCM, key in the OS keychain); never sent anywhere" : "Not saved: the web preview keeps conversations for this session only"}
          style={{ fontSize: 9.5, letterSpacing: ".06em", color: "var(--tx-3)" }}
        >
          {!loaded ? "opening…" : persisted ? "encrypted on this device" : "this session only"}
        </span>
        <button
          type="button"
          className="mono"
          data-testid="chat-history-manage"
          aria-expanded={manage}
          aria-controls="chat-history-manage-panel"
          onClick={() => setManage((v) => !v)}
          style={{ fontSize: 10, letterSpacing: ".06em", textTransform: "uppercase", color: "var(--tx-2)", background: "transparent", border: "1px solid var(--line-1)", borderRadius: 999, padding: "3px 9px", cursor: "pointer" }}
        >
          Manage
        </button>
      </div>
      {error && (
        <p role="alert" data-testid="chat-history-error" style={{ fontSize: 12, color: "var(--danger)", margin: 0 }}>
          {error}
        </p>
      )}
      {unreadable.length > 0 && (
        <p data-testid="chat-history-unreadable" style={{ fontSize: 12, color: "var(--tx-2)", margin: 0 }}>
          {unreadable.length === 1 ? "1 saved conversation does not open" : `${unreadable.length} saved conversations do not open`} (changed, moved, or sealed under
          a key this device no longer has). It is not shown and is left untouched.
        </p>
      )}
      {manage && (
        <div id="chat-history-manage-panel" data-testid="chat-history-manage-panel" style={{ display: "flex", flexDirection: "column", gap: 8, padding: 10, border: "1px solid var(--line-1)", borderRadius: "var(--r-1)" }}>
          {retentionUnreadable && (
            <p role="alert" data-testid="chat-history-retention-unreadable" style={{ fontSize: 12, color: "var(--danger)", margin: 0 }}>
              The saved retention setting does not open (it was changed or damaged), so nothing is erased by age. Choose a setting to replace it.
            </p>
          )}
          <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12.5 }}>
            <span style={{ flex: 1 }}>Keep conversations</span>
            <select
              data-testid="chat-history-retention"
              disabled={!persisted}
              value={retentionDays === null ? "" : String(retentionDays)}
              onChange={(e) => void setHistoryRetention(e.target.value === "" ? null : Number(e.target.value))}
              style={{ fontSize: 12.5 }}
            >
              {RETENTION_CHOICES.map((c) => (
                <option key={c.label} value={c.days === null ? "" : String(c.days)}>
                  {c.label}
                </option>
              ))}
              {retentionDays !== null && !RETENTION_CHOICES.some((c) => c.days === retentionDays) && <option value={String(retentionDays)}>{retentionDays} days</option>}
            </select>
          </label>
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            <span style={{ fontSize: 12.5 }}>Export every conversation as an encrypted journal file</span>
            <input className="input" type="password" data-testid="chat-history-export-pass" aria-label="Export passphrase" placeholder="Passphrase (12+ characters)" value={pass} onChange={(e) => setPass(e.target.value)} style={{ height: 30, fontSize: 12.5 }} />
            <input className="input" type="password" data-testid="chat-history-export-pass2" aria-label="Repeat the export passphrase" placeholder="Repeat the passphrase" value={pass2} onChange={(e) => setPass2(e.target.value)} style={{ height: 30, fontSize: 12.5 }} />
            <button type="button" className="btn btn-sm btn-ghost" data-testid="chat-history-export" disabled={threads.length === 0} onClick={() => void exportAll()}>
              Export (encrypted)
            </button>
          </div>
          {!confirmErase ? (
            <button type="button" className="btn btn-sm btn-danger" data-testid="chat-history-erase-all" disabled={busy} onClick={() => setConfirmErase(true)}>
              Delete all history
            </button>
          ) : (
            <span style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
              <span style={{ fontSize: 12, color: "var(--danger)", flex: 1 }}>Erase every conversation, the current one too, and the key that opens them? This cannot be undone. Deletes are device-only: an OS backup made earlier (such as Time Machine, which includes the keychain) may still hold the old key and files, and restoring it may reopen deleted conversations.</span>
              <button type="button" className="btn btn-sm btn-danger" data-testid="chat-history-erase-all-confirm" onClick={() => void eraseAll()}>
                Erase
              </button>
              <button type="button" className="btn btn-sm btn-ghost" onClick={() => setConfirmErase(false)}>
                Cancel
              </button>
            </span>
          )}
          {note && (
            <p data-testid="chat-history-note" style={{ fontSize: 12, color: "var(--tx-2)", margin: 0 }}>
              {note}
            </p>
          )}
        </div>
      )}
      <div id="chat-history-list" role="tabpanel" aria-labelledby={"chat-history-tab-" + tab} data-testid="chat-history-list" style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        {tab === "chats" && chats.length === 0 && (
          <p style={{ fontSize: 12.5, color: "var(--tx-3)", margin: "4px 2px" }}>
            {query ? "No conversation matches." : persisted ? "No earlier conversations on this device yet. Each conversation is sealed here once you send a message." : "No earlier conversations yet."}
          </p>
        )}
        {tab === "chats" &&
          chats.map((t) => {
            const current = t.id === currentId;
            return (
              <div key={t.id} style={{ display: "flex", alignItems: "center", gap: 4 }}>
                <button
                  type="button"
                  data-testid="chat-history-item"
                  data-thread-id={t.id}
                  aria-current={current ? "true" : undefined}
                  disabled={busy && !current}
                  onClick={() => open(t.id)}
                  style={{ ...rowStyle, background: current ? "var(--srf-2)" : "transparent" }}
                >
                  <span style={{ flex: 1, minWidth: 0 }}>
                    <span style={{ display: "block", fontSize: 12.5, fontWeight: 500, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{t.title}</span>
                    <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)" }}>
                      {t.msgs.length} messages · {when(t.updatedAt)}
                      {current ? " · current" : ""}
                    </span>
                  </span>
                </button>
                {!current && (
                  <button
                    type="button"
                    className="btn btn-ghost btn-icon"
                    data-testid="chat-history-forget"
                    aria-label={"Erase “" + t.title + "” from this device"}
                    data-tip="Erase from this device"
                    onClick={() => void store.forgetChat(t.id)}
                    style={{ width: 26, height: 26, fontSize: 13, color: "var(--tx-3)" }}
                  >
                    ×
                  </button>
                )}
              </div>
            );
          })}
        {tab === "projects" && projects.length === 0 && (
          <p style={{ fontSize: 12.5, color: "var(--tx-3)", margin: "4px 2px" }}>
            {query ? "No project matches." : "No projects yet. A project starts when you accept a brief from “Plan it first”."}
          </p>
        )}
        {tab === "projects" &&
          projects.map((p) => (
            <button
              key={p.threadId}
              type="button"
              data-testid="chat-history-project"
              data-thread-id={p.threadId}
              aria-current={p.threadId === currentId ? "true" : undefined}
              disabled={busy && p.threadId !== currentId}
              onClick={() => open(p.threadId)}
              style={{ ...rowStyle, background: p.threadId === currentId ? "var(--srf-2)" : "transparent" }}
            >
              <span style={{ flex: 1, minWidth: 0 }}>
                <span style={{ display: "block", fontSize: 12.5, fontWeight: 500, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{p.goal}</span>
                <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)" }}>
                  {p.track} · {p.workflow} · {when(p.updatedAt)}
                </span>
              </span>
            </button>
          ))}
      </div>
      <p data-testid="chat-history-erase-scope" style={{ fontSize: 11, lineHeight: 1.5, color: "var(--tx-3)", margin: 0 }}>
        Deletes are device-only. Erasing a conversation makes it unreadable on this device, but an OS backup made earlier (such as Time Machine, which includes the keychain) may still hold the old key and file, and restoring that backup may reopen it.
      </p>
      <p style={{ fontSize: 11, lineHeight: 1.5, color: "var(--tx-3)", margin: 0 }}>
        Reopening shows the conversation as it was kept. Hermes starts a fresh agent session for it, so with the agent loop on it does not
        see the earlier turns unless you restate them.
      </p>
    </div>
  );
}
