// =====================================================================
// HUP-S10.2 follow-up (US-10.2) — sheets Hermes wrote, with Undo, in Journal > Sheets.
//
// Hermes's `sheet_write` tool writes a CSV or XLSX file into a folder the member granted, and the
// sidecar checkpoints every such write (HUP-S2.9). The chat shows each change as a card with Undo;
// this list gathers the sheet writes of this app session in the Sheets view too, so a member who
// opens Journal > Sheets can undo one there. Each row is the same UndoCard the chat shows, and Undo
// goes through the same path (agentUndo.undoChange -> Rust hermes_undo_step -> the sidecar's
// checkpoint store): a refusal (the file changed since, the step was pruned) stays a refusal with
// its reason (Rule 1).
//
// Data source (Rule 7): the agent undo slice, filled only from the sidecar's `tool_result` events
// that name a checkpoint (src/agent/fileChanges.ts parseFileChange).
// =====================================================================
import { FileChangeCard } from "../../components/FileChangeCard";
import type { UndoCard } from "../../shell/slices/agentUndo";

export const SHEET_WRITE_TOOL = "sheet_write";

export const NO_AGENT_SHEETS =
  "Hermes has not written a sheet in this session. When it writes one in a folder you granted, it shows here with Undo.";

/** The sheet writes among the cards, newest first. */
export function sheetWriteCards(cards: readonly UndoCard[]): UndoCard[] {
  return cards.filter((c) => c.tool === SHEET_WRITE_TOOL).reverse();
}

export function AgentSheetChanges({ cards, onUndo }: { cards: readonly UndoCard[]; onUndo: (session: string, seq: number) => void }) {
  const sheets = sheetWriteCards(cards);
  return (
    <section data-testid="agent-sheet-changes" aria-label="Sheets Hermes wrote" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <span style={{ fontSize: 12, fontWeight: 600, color: "var(--tx-1)" }}>Sheets Hermes wrote</span>
      {sheets.length === 0 ? (
        <span data-testid="agent-sheet-changes-empty" style={{ fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 }}>
          {NO_AGENT_SHEETS}
        </span>
      ) : (
        sheets.map((c) => <FileChangeCard key={c.session + ":" + c.seq} card={c} onUndo={onUndo} />)
      )}
    </section>
  );
}
