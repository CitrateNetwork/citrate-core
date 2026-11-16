// =====================================================================
// ChatCommandBar: the Hermes chat's command bar (Cmd K / Ctrl K) and its slash commands
// (FL design board 01, "Keyboard first"). Every command runs something the chat already does;
// none is a stand-in. `/run <workflow>` keeps going to the store's track workflow runner.
// =====================================================================
import { useEffect, useMemo, useRef, useState } from "react";

export interface ChatCommand {
  id: string;
  slash: string;
  label: string;
  run: () => void;
  /** Text put in the composer instead of running (for commands that take an argument). */
  insert?: string;
}

export interface CommandDeps {
  live: boolean;
  stop: () => void;
  newChat: () => void;
  history: () => void;
  model: () => void;
  plan: () => void;
  monitor: () => void;
  focus: () => void;
  /** Open "What Hermes can see" (absent = not offered). */
  context?: () => void;
}

/** The chat's commands; Stop is listed only while a run is live. */
export function chatCommands(d: CommandDeps): ChatCommand[] {
  const list: ChatCommand[] = [
    { id: "new", slash: "/new", label: "Start a new chat", run: d.newChat },
    { id: "history", slash: "/history", label: "Earlier chats and projects", run: d.history },
    { id: "model", slash: "/model", label: "Switch model", run: d.model },
    { id: "plan", slash: "/plan", label: "Plan it first", run: d.plan },
    { id: "monitor", slash: "/monitor", label: "Open the Activity monitor", run: d.monitor },
    { id: "run", slash: "/run", label: "Run a track workflow (/run <workflow>)", run: d.focus, insert: "/run " },
  ];
  if (d.context) list.splice(3, 0, { id: "context", slash: "/see", label: "What Hermes can see here", run: d.context });
  if (d.live) list.unshift({ id: "stop", slash: "/stop", label: "Stop this run (Esc)", run: d.stop });
  return list;
}

/** Commands whose slash word starts with what is typed ("/" alone lists all). */
export function slashMatches(cmds: ChatCommand[], typed: string): ChatCommand[] {
  const t = typed.trim().toLowerCase();
  if (!t.startsWith("/")) return [];
  const word = t.split(/\s+/)[0];
  return cmds.filter((c) => c.slash.startsWith(word) || (word.length > 1 && c.slash === word));
}

/** Filter by words in the label or the slash word. */
export function filterCommands(cmds: ChatCommand[], q: string): ChatCommand[] {
  const t = q.trim().toLowerCase().replace(/^\//, "");
  if (!t) return cmds;
  return cmds.filter((c) => c.label.toLowerCase().includes(t) || c.slash.slice(1).startsWith(t));
}

export function ChatCommandBar({ commands, onRun, onClose }: { commands: ChatCommand[]; onRun: (c: ChatCommand) => void; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const input = useRef<HTMLInputElement | null>(null);
  const shown = useMemo(() => filterCommands(commands, q), [commands, q]);
  useEffect(() => {
    input.current?.focus();
  }, []);
  useEffect(() => {
    setSel(0);
  }, [q]);
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Chat commands"
      data-testid="command-bar"
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          onClose();
        } else if (e.key === "ArrowDown") {
          e.preventDefault();
          setSel((i) => Math.min(shown.length - 1, i + 1));
        } else if (e.key === "ArrowUp") {
          e.preventDefault();
          setSel((i) => Math.max(0, i - 1));
        } else if (e.key === "Enter" && shown[sel]) {
          e.preventDefault();
          onRun(shown[sel]);
        }
      }}
      style={{ margin: "10px 16px 0", border: "1px solid var(--line-2)", borderRadius: 8, background: "var(--srf-1)", boxShadow: "0 8px 24px rgba(0,0,0,.12)", flexShrink: 0 }}
    >
      <label className="sr-only" htmlFor="chat-command-q">
        Type a command
      </label>
      <input
        id="chat-command-q"
        ref={input}
        className="input"
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder="Type a command"
        role="combobox"
        aria-expanded="true"
        aria-controls="chat-command-list"
        aria-activedescendant={shown[sel] ? "chat-cmd-" + shown[sel].id : undefined}
        style={{ width: "100%", border: 0, borderBottom: "1px solid var(--line-1)", borderRadius: "8px 8px 0 0" }}
      />
      <ul id="chat-command-list" role="listbox" style={{ listStyle: "none", margin: 0, padding: 4, maxHeight: 260, overflow: "auto" }}>
        {shown.length === 0 && <li style={{ padding: "6px 10px", fontSize: 12.5, color: "var(--tx-3)" }}>No command matches.</li>}
        {shown.map((c, i) => (
          <li key={c.id} id={"chat-cmd-" + c.id} role="option" aria-selected={i === sel}>
            <button
              type="button"
              data-testid={"cmd-" + c.id}
              tabIndex={-1}
              onMouseEnter={() => setSel(i)}
              onClick={() => onRun(c)}
              style={{ width: "100%", display: "flex", gap: 10, alignItems: "center", textAlign: "left", font: "inherit", fontSize: 13, padding: "7px 10px", minHeight: 30, border: 0, borderRadius: 6, cursor: "pointer", color: "var(--tx-1)", background: i === sel ? "var(--srf-2)" : "transparent" }}
            >
              <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)", minWidth: 70 }}>
                {c.slash}
              </span>
              {c.label}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
