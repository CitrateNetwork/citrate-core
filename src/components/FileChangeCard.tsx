// =====================================================================
// citrate-core — the file-change card under an agent reply (HUP-S2.9)
//
// A pure view over one UndoCard: what the agent changed (in a folder the member granted), an Undo
// button while the change can be undone, and the result in words. A refusal (the file changed
// since, the step was pruned) is shown as an alert with the reason and Undo can be tried again;
// nothing here claims an undo that did not happen (Rule 1).
// =====================================================================
import type { UndoCard } from "../shell/slices/agentUndo";

const VERB: Record<string, string> = { fs_write: "Wrote", fs_edit: "Edited", fs_delete: "Deleted", fs_rename: "Renamed", file_write: "Wrote", sheet_write: "Wrote sheet" };

export function FileChangeCard({
  card,
  onUndo,
  onDiff,
}: {
  card: UndoCard;
  onUndo: (session: string, seq: number) => void;
  /** HUP-S5.4: open the Code and diff pop-out on this change (desktop app only). */
  onDiff?: (session: string, seq: number) => void;
}) {
  const verb = VERB[card.tool] ?? "Changed";
  const what = card.tool === "fs_rename" && card.paths.length === 2 ? `${card.paths[0]} to ${card.paths[1]}` : card.paths.join(", ");
  const canUndo = card.state === "applied" || card.state === "refused" || card.state === "failed";
  const bad = card.state === "refused" || card.state === "failed";
  return (
    <div
      data-testid="file-change-card"
      style={{ display: "flex", flexDirection: "column", gap: 4, border: "1px solid var(--line-2)", borderRadius: 8, padding: "8px 10px", background: "var(--srf-1)", fontSize: 12 }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <span style={{ flex: 1, minWidth: 0, color: "var(--tx-1)", overflowWrap: "anywhere" }}>
          <span style={{ color: card.state === "undone" ? "var(--tx-3)" : "var(--accent-text)" }}>{verb}</span>{" "}
          <span className="mono">{what}</span>
        </span>
        {onDiff ? (
          <button className="btn btn-sm btn-ghost" data-testid="file-change-diff" onClick={() => onDiff(card.session, card.seq)} title="Show what this change did, line by line">
            Diff
          </button>
        ) : null}
        {card.state !== "undone" ? (
          <button
            className="btn btn-sm"
            data-testid="file-change-undo"
            disabled={!canUndo}
            onClick={() => onUndo(card.session, card.seq)}
            title="Restore what was there before this change"
          >
            {card.state === "undoing" ? "Undoing…" : "Undo"}
          </button>
        ) : null}
      </div>
      {card.note ? (
        <span data-testid="file-change-note" role={bad ? "alert" : "status"} style={{ color: bad ? "var(--danger)" : "var(--tx-2)" }}>
          {card.note}
        </span>
      ) : null}
    </div>
  );
}
