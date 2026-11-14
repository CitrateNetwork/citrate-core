// 0.5.4 — prior conversations and projects, reachable from the Hermes chat (owner request
// 2026-11-14): list, search, reopen.
//
// Data sources (Rule 7):
//   Conversations — the chat history slice (src/shell/slices/chatHistory.ts): the threads of
//     AppState.chatMsgs this device has had, kept in localStorage `citrate.hermes.chatHistory.v1`.
//   Projects — derived from the same threads: each conversation holding an accepted "Plan it
//     first" brief (ChatMsg.brief, written by store.acceptBrief from the sidecar's brief_create).
// Nothing here is fetched from a server and nothing is invented: an empty history says so.
import { useState } from "react";
import { chatHistory, projectsFrom, searchProjects, searchThreads } from "../shell/slices/chatHistory";
import type { Store } from "../shell/store";

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
  const { threads } = chatHistory.use();
  const [tab, setTab] = useState<Tab>("chats");
  const [query, setQuery] = useState("");
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
        <span className="mono" title="Kept in this app's local storage; never sent anywhere" style={{ fontSize: 9.5, letterSpacing: ".06em", color: "var(--tx-3)" }}>
          on this device
        </span>
      </div>
      <div id="chat-history-list" role="tabpanel" aria-labelledby={"chat-history-tab-" + tab} data-testid="chat-history-list" style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        {tab === "chats" && chats.length === 0 && (
          <p style={{ fontSize: 12.5, color: "var(--tx-3)", margin: "4px 2px" }}>
            {query ? "No conversation matches." : "No earlier conversations on this device yet. Each conversation is kept here once you send a message."}
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
                    aria-label={"Remove “" + t.title + "” from this device's history"}
                    data-tip="Remove from history"
                    onClick={() => store.forgetChat(t.id)}
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
      <p style={{ fontSize: 11, lineHeight: 1.5, color: "var(--tx-3)", margin: 0 }}>
        Reopening shows the conversation as it was kept. Hermes starts a fresh agent session for it, so with the agent loop on it does not
        see the earlier turns unless you restate them.
      </p>
    </div>
  );
}
