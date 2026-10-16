// =====================================================================
// citrate-core — the main window's side of the Code and diff pop-out (HUP-S5.4)
//
// Answers the pop-out's requests with the real seams: the agent session's checkpointed steps
// (bridge.agentHarness.checkpoints, Rust `hermes_checkpoints`) and one step's diff
// (bridge.agentHarness.checkpointDiff, Rust `hermes_checkpoint_diff`). Which session to show first
// is the one a "Diff" button named, else the session of the most recent agent file change.
// Desktop app only; the web preview has no windows to open.
// =====================================================================
import { bridge } from "../bridge";
import { agentUndo } from "../shell/slices/agentUndo";
import type { CheckpointList } from "../agent/fileChanges";
import { createDiffHost, type DiffFocus, type DiffOps } from "./diffChannel";
import type { StepDiff } from "./diffModel";
import type { BridgeTransport } from "./bridge";

let pendingFocus: DiffFocus | null = null;
let hostPromise: Promise<{ focus(session: string, seq: number | null): Promise<void>; close(): void }> | null = null;

export interface DiffApi {
  checkpoints(id: string): Promise<CheckpointList>;
  checkpointDiff(id: string, seq: number): Promise<StepDiff>;
}

/** The pop-out's operations, wired to `api` and the current agent session. */
export function diffOps(api: DiffApi = bridge.agentHarness, currentSession: () => string | null = () => agentUndo.get().session): DiffOps {
  return {
    initial: async () => {
      const f = pendingFocus;
      pendingFocus = null;
      if (f) return f;
      const s = currentSession();
      return s ? { session: s, seq: null } : null;
    },
    steps: ({ session }) => api.checkpoints(session),
    diff: ({ session, seq }) => api.checkpointDiff(session, seq),
  };
}

/** Start answering the pop-out (once). */
export function startDiffHost(transport: () => Promise<BridgeTransport>): void {
  hostPromise ??= transport().then((t) => createDiffHost(t, diffOps()));
  hostPromise.catch(() => {
    hostPromise = null;
  });
}

/** Remember which step the pop-out should open; tell an open pop-out right away. */
export async function focusDiff(session: string, seq: number | null): Promise<void> {
  pendingFocus = { session, seq };
  const host = hostPromise ? await hostPromise.catch(() => null) : null;
  await host?.focus(session, seq).catch(() => undefined);
}

/** Test helper. */
export function resetDiffFocus(): void {
  pendingFocus = null;
}
