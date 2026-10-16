// =====================================================================
// citrate-core — agent undo slice (HUP-S2.9)
//
// The file changes the Hermes sidecar's file tools made in this app session, as cards on the chat
// message that made them, plus the Activity monitor's undo panel for the current agent session.
// Every state traces to a real event: a card is created only from a sidecar `tool_result` that
// names its checkpoint, and changes state only from the sidecar's undo answer (through Rust). A
// refusal stays a refusal, with the sidecar's reason (Rule 1). Undo itself never runs a tool and
// never signs: it restores the member's own prior file content.
// =====================================================================
import { createSlice } from "./createSlice";
import { describeUndo, type CheckpointList, type FileChange, type UndoOutcome } from "../../agent/fileChanges";
import { MAX_PANEL_STEPS, type UndoPanel } from "../../popout/undoPanel";

export interface UndoCard extends FileChange {
  /** The chat message the change belongs to. */
  msgId: string;
  /** applied: changed, can be undone; refused/failed: an undo did not happen (can be tried again). */
  state: "applied" | "undoing" | "undone" | "refused" | "failed";
  note: string | null;
}

/** The calls this slice needs (bridge.agentHarness in the app; fakes in tests). */
export interface UndoApi {
  checkpoints(id: string): Promise<CheckpointList>;
  undoStep(id: string, seq: number): Promise<UndoOutcome>;
  undoSession(id: string): Promise<UndoOutcome>;
}

export interface AgentUndoState {
  cards: UndoCard[];
  /** The agent session of the most recent file change. */
  session: string | null;
  panel: UndoPanel;
}

/** Cards kept for this app session (the oldest are dropped; undo stays possible from the monitor). */
export const MAX_CARDS = 50;

const NO_SESSION_NOTE = "No agent file changes yet. When the agent changes a file in a folder you granted, it shows here with Undo.";

const INITIAL: AgentUndoState = {
  cards: [],
  session: null,
  panel: { session: null, enabled: false, note: NO_SESSION_NOTE, busy: false, steps: [], last: null },
};

export const agentUndo = createSlice<AgentUndoState>(INITIAL);

/** Test helper: back to the initial state. */
export function resetAgentUndo(): void {
  agentUndo.set(INITIAL);
}

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function recordFileChange(change: FileChange, msgId: string): void {
  agentUndo.set((s) => {
    if (s.cards.some((c) => c.session === change.session && c.seq === change.seq)) return { session: change.session };
    const card: UndoCard = { ...change, msgId, state: "applied", note: null };
    return { cards: s.cards.concat([card]).slice(-MAX_CARDS), session: change.session };
  });
}

function patchCards(session: string, seqs: (seq: number) => boolean, patch: Partial<UndoCard>): void {
  agentUndo.set((s) => ({ cards: s.cards.map((c) => (c.session === session && seqs(c.seq) ? { ...c, ...patch } : c)) }));
}

function setPanel(patch: Partial<UndoPanel>): void {
  agentUndo.set((s) => ({ panel: { ...s.panel, ...patch } }));
}

/** Re-read the current session's steps from the sidecar for the Activity monitor. */
export async function refreshUndoPanel(api: UndoApi): Promise<void> {
  const session = agentUndo.get().session;
  if (!session) {
    setPanel({ session: null, enabled: false, steps: [], note: NO_SESSION_NOTE });
    return;
  }
  try {
    const l = await api.checkpoints(session);
    setPanel({
      session,
      enabled: l.enabled,
      note: l.enabled ? null : l.note ?? "undo is not available",
      steps: l.steps.slice(0, MAX_PANEL_STEPS).map((st) => ({ seq: st.seq, status: st.status, paths: st.paths })),
    });
  } catch (e) {
    setPanel({ session, enabled: false, steps: [], note: "The recent changes could not be read: " + errText(e) });
  }
}

/** Undo one step (from its card or the monitor). */
export async function undoChange(api: UndoApi, session: string, seq: number): Promise<void> {
  patchCards(session, (n) => n === seq, { state: "undoing", note: null });
  setPanel({ busy: true });
  try {
    const o = await api.undoStep(session, seq);
    const text = describeUndo(o);
    patchCards(session, (n) => n === seq, { state: o.ok ? "undone" : o.kind === "already_undone" ? "undone" : "refused", note: text });
    setPanel({ last: { ok: o.ok, text } });
  } catch (e) {
    const text = "Undo failed: " + errText(e);
    patchCards(session, (n) => n === seq, { state: "failed", note: text });
    setPanel({ last: { ok: false, text } });
  }
  setPanel({ busy: false });
  await refreshUndoPanel(api);
}

/** Undo every change of the session not undone yet (all or nothing). */
export async function undoSession(api: UndoApi, session: string): Promise<void> {
  setPanel({ busy: true });
  try {
    const o = await api.undoSession(session);
    const text = describeUndo(o);
    if (o.ok) patchCards(session, (n) => o.undone.includes(n), { state: "undone", note: text });
    setPanel({ last: { ok: o.ok, text } });
  } catch (e) {
    setPanel({ last: { ok: false, text: "Undo failed: " + errText(e) } });
  }
  setPanel({ busy: false });
  await refreshUndoPanel(api);
}
