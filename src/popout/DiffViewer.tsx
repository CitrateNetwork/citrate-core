// =====================================================================
// citrate-core — Code and diff pop-out view (HUP-S5.4 over HUP-S2.9)
//
// The agent session's checkpointed file changes, newest first, and for the selected one what it
// did to each path: a line diff when both sides are text (or one side did not exist), otherwise a
// plain sentence per side (binary, too large, a link, or why the result is not shown). Read-only:
// undo stays in the chat card and the Activity monitor. Every request goes over the diff channel to
// the main window; this view holds no app commands.
// HUP-S10.6 (a11y): a <main> landmark with an <h1>; the change list is a list of toggle buttons
// (aria-pressed); each file is a region named by its path; added and removed lines say so in text
// for screen readers, not only by colour; loading is a polite status, failures are alerts.
// =====================================================================
import { useEffect, useId, useState, type CSSProperties } from "react";
import type { CheckpointList } from "../agent/fileChanges";
import type { DiffClient, DiffFocus } from "./diffChannel";
import { describeSide, diffStats, hunks, lineDiff, type DiffLine, type FileDiff, type StepDiff } from "./diffModel";

const shell: CSSProperties = {
  minHeight: "100vh",
  boxSizing: "border-box",
  padding: "14px 16px",
  background: "var(--srf-0)",
  color: "var(--tx-1)",
  fontFamily: "var(--font-sans)",
  fontSize: 13,
  display: "flex",
  flexDirection: "column",
  gap: 10,
};
const note: CSSProperties = { margin: 0, fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 };
const mono: CSSProperties = { fontFamily: "var(--font-mono)", fontSize: 11.5 };

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

function stepLabel(s: CheckpointList["steps"][number]): string {
  const what = s.paths.length === 1 ? s.paths[0] : `${s.paths.length} files`;
  return `Change ${s.seq}: ${what}${s.status === "committed" ? "" : ` (${s.status})`}`;
}

function Lines({ lines, path }: { lines: DiffLine[]; path: string }) {
  const groups = hunks(lines);
  if (groups.length === 0) return <p style={note}>No line changed (only the file mode or nothing at all).</p>;
  return (
    <table style={{ ...mono, borderCollapse: "collapse", width: "100%" }} data-testid="diff-lines">
      <caption className="sr-only">Line changes in {path}</caption>
      <thead className="sr-only">
        <tr>
          <th scope="col">Line before</th>
          <th scope="col">Line after</th>
          <th scope="col">Change</th>
          <th scope="col">Text</th>
        </tr>
      </thead>
      <tbody>
        {groups.map((g, gi) => (
          <HunkRows key={gi} lines={g} first={gi === 0} />
        ))}
      </tbody>
    </table>
  );
}

function HunkRows({ lines, first }: { lines: DiffLine[]; first: boolean }) {
  return (
    <>
      {!first && (
        <tr data-testid="diff-gap">
          <td colSpan={4} style={{ color: "var(--tx-3)", padding: "2px 6px" }}>
            …
            <span className="sr-only"> unchanged lines not shown</span>
          </td>
        </tr>
      )}
      {lines.map((l, i) => {
        const bg = l.op === "add" ? "var(--ok-bg)" : l.op === "del" ? "var(--danger-bg, var(--srf-2))" : "transparent";
        const sign = l.op === "add" ? "+" : l.op === "del" ? "-" : " ";
        const said = l.op === "add" ? "added" : l.op === "del" ? "removed" : "unchanged";
        return (
          <tr key={i} data-testid={`diff-line-${l.op}`} style={{ background: bg }}>
            <td style={{ width: 44, textAlign: "right", color: "var(--tx-3)", padding: "0 6px", userSelect: "none" }}>{l.oldNo ?? ""}</td>
            <td style={{ width: 44, textAlign: "right", color: "var(--tx-3)", padding: "0 6px", userSelect: "none" }}>{l.newNo ?? ""}</td>
            <td style={{ width: 16, color: l.op === "add" ? "var(--ok)" : l.op === "del" ? "var(--danger)" : "var(--tx-3)" }}>
              <span aria-hidden="true">{sign}</span>
              <span className="sr-only">{said}</span>
            </td>
            <td style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere", padding: "0 6px" }}>{l.text}</td>
          </tr>
        );
      })}
    </>
  );
}

function FileSection({ f }: { f: FileDiff }) {
  const id = useId();
  const textish = (s: FileDiff["before"]) => s.kind === "text" || s.kind === "absent";
  const comparable = textish(f.before) && textish(f.after) && !(f.before.kind === "absent" && f.after.kind === "absent");
  const diff = comparable
    ? lineDiff(f.before.kind === "text" ? f.before.text : "", f.after.kind === "text" ? f.after.text : "")
    : null;
  const st = diff ? diffStats(diff.lines) : null;
  return (
    <section aria-labelledby={id} data-testid="diff-file" style={{ border: "1px solid var(--line-1)", borderRadius: 8, padding: "8px 10px", display: "flex", flexDirection: "column", gap: 6 }}>
      <h2 id={id} style={{ ...mono, fontSize: 12, fontWeight: 500, margin: 0, overflowWrap: "anywhere" }}>
        {f.path}
      </h2>
      {st && (
        <p style={note} data-testid="diff-stats">
          {st.added} added, {st.removed} removed
          {f.before.kind === "absent" ? " (new file)" : f.after.kind === "absent" ? " (file removed)" : ""}
        </p>
      )}
      {diff ? (
        <>
          {!diff.exact && <p style={note}>The changed part is large, so it is shown as removed then added rather than line by line.</p>}
          <Lines lines={diff.lines} path={f.path} />
        </>
      ) : (
        <>
          {f.before.kind !== "text" && <p style={note} data-testid="diff-before-note">Before: {describeSide(f.before, "before")}</p>}
          {f.after.kind !== "text" && <p style={note} data-testid="diff-after-note">After: {describeSide(f.after, "after")}</p>}
          {f.before.kind === "text" && <p style={note}>Before: text, {f.before.text.split("\n").length} lines.</p>}
          {f.after.kind === "text" && <p style={note}>After: text, {f.after.text.split("\n").length} lines.</p>}
        </>
      )}
    </section>
  );
}

export function DiffViewer({ client, focus }: { client: DiffClient; focus: DiffFocus | null }) {
  const h1 = useId();
  const [session, setSession] = useState<string | null | undefined>(undefined);
  const [want, setWant] = useState<number | null>(null);
  const [list, setList] = useState<CheckpointList | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [diff, setDiff] = useState<StepDiff | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  // Where to start: what the main window asked for, else the latest agent session.
  useEffect(() => {
    let live = true;
    client
      .call("initial", {})
      .then((f) => {
        if (!live) return;
        setSession(f ? f.session : null);
        setWant(f ? f.seq : null);
      })
      .catch((e) => live && setError(errText(e)));
    return () => {
      live = false;
    };
  }, [client]);

  // A "Diff" button pressed while the window is open.
  useEffect(() => {
    if (!focus) return;
    setSession(focus.session);
    setWant(focus.seq);
  }, [focus]);

  useEffect(() => {
    if (!session) return;
    let live = true;
    setError(null);
    client
      .call("steps", { session })
      .then((l) => {
        if (!live) return;
        setList(l);
        const pick = want !== null && l.steps.some((s) => s.seq === want) ? want : (l.steps[0]?.seq ?? null);
        setSelected(pick);
      })
      .catch((e) => live && setError(errText(e)));
    return () => {
      live = false;
    };
  }, [client, session, want]);

  useEffect(() => {
    if (!session || selected === null) {
      setDiff(null);
      return;
    }
    let live = true;
    setLoading(true);
    setError(null);
    client
      .call("diff", { session, seq: selected })
      .then((d) => live && setDiff(d))
      .catch((e) => live && setError(errText(e)))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
  }, [client, session, selected]);

  return (
    <main data-register="instrument" aria-labelledby={h1} style={shell} data-testid="diff-viewer">
      <h1 id={h1} style={{ fontSize: 14, fontWeight: 500, margin: 0 }}>
        Code and diff
      </h1>
      <p style={note}>What the agent's file changes did. Read-only: undo a change from its card in the chat or from the Activity monitor.</p>
      {error && (
        <p role="alert" style={{ ...note, color: "var(--danger)" }} data-testid="diff-error">
          {error}
        </p>
      )}
      {session === undefined && !error && (
        <p role="status" aria-live="polite" style={note}>
          Asking the main window…
        </p>
      )}
      {session === null && (
        <p role="status" style={note} data-testid="diff-empty">
          No agent file changes yet. When the agent changes a file in a folder you granted, its changes show here.
        </p>
      )}
      {list && !list.enabled && (
        <p role="status" style={note} data-testid="diff-disabled">
          {list.note ?? "Undo checkpoints are not enabled, so there are no changes to show."}
        </p>
      )}
      {list && list.enabled && list.steps.length === 0 && (
        <p role="status" style={note}>
          This session has no recorded file changes.
        </p>
      )}
      {list && list.steps.length > 0 && (
        <nav aria-label="Agent file changes">
          <ul style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexWrap: "wrap", gap: 6 }}>
            {list.steps.map((s) => (
              <li key={s.seq}>
                <button
                  type="button"
                  className="btn btn-sm"
                  data-testid="diff-step"
                  aria-pressed={s.seq === selected}
                  onClick={() => setSelected(s.seq)}
                  style={{ ...mono, maxWidth: 360, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                >
                  {stepLabel(s)}
                </button>
              </li>
            ))}
          </ul>
        </nav>
      )}
      {loading && (
        <p role="status" aria-live="polite" style={note}>
          Loading the change…
        </p>
      )}
      {diff && !diff.ok && (
        <p role="alert" style={note} data-testid="diff-refused">
          {diff.reason ?? "The change could not be shown."}
        </p>
      )}
      {diff && diff.ok && (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }} data-testid="diff-files">
          {diff.status === "undone" && (
            <p style={note} data-testid="diff-undone">
              This change was undone. The file as it was before the change is shown; its result is no longer on disk.
            </p>
          )}
          {diff.files.map((f) => (
            <FileSection key={f.path} f={f} />
          ))}
        </div>
      )}
    </main>
  );
}
