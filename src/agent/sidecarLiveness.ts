// =====================================================================
// 0.5.4 (owner, 2026-11-14): "a warning pops up saying [Hermes] isn't running when it is."
//
// Root cause. The chat provider (store.rebuildProvider) read the Hermes status ONCE, when it was
// built: at launch, or after a Settings change. Hermes is started later (Agent › Start, Node ›
// Services), and nothing rebuilt the provider when it came up, so chat kept the "Hermes is down"
// provider and toasted SIDECAR_DOWN_NOTICE on every message while the Agent page said running.
// Two smaller faults fed the same warning: a status read that FAILED counted as "not running"
// (`.catch(() => false)`), and with the sidecar loop switched off in Settings the toast still said
// Hermes was not running, whatever its real state.
//
// The fix (store.ts): the provider is rebuilt whenever the Agent slice sees Hermes start or stop,
// and a message about to go out on the no-tools fallback re-reads the status first, so the toast
// is shown only from a fresh read, and says which of the three cases it is.
// =====================================================================

/** The harness status call (bridge.agentHarness.status). */
export type SidecarStatusFn = () => Promise<{ running: boolean }>;

/** One fresh read of the Hermes sidecar. `unreadable` = the status call itself failed. */
export type SidecarReading = { up: true } | { up: false; why: "stopped" } | { up: false; why: "unreadable"; error: string };

/** Why chat is on the no-tools local fallback. */
export type SidecarDownReason = { why: "stopped" } | { why: "loop_off" } | { why: "unreadable"; error: string };

/** Read the sidecar once. Never throws: a failed read is `unreadable` with its error, not "stopped". */
export async function readSidecar(status: SidecarStatusFn): Promise<SidecarReading> {
  try {
    const st = await status();
    return st.running === true ? { up: true } : { up: false, why: "stopped" };
  } catch (e) {
    return { up: false, why: "unreadable", error: errorText(e) };
  }
}

/** Why the fallback is in use: the member's loop switch first (no read is made then), else the read. */
export function downReason(loopOn: boolean, reading: SidecarReading | null): SidecarDownReason {
  if (!loopOn) return { why: "loop_off" };
  if (reading && !reading.up && reading.why === "unreadable") return { why: "unreadable", error: reading.error };
  return { why: "stopped" };
}

/** Whether two reasons say the same thing (a rebuild is needed only when they differ). */
export function sameReason(a: SidecarDownReason, b: SidecarDownReason): boolean {
  if (a.why !== b.why) return false;
  return a.why !== "unreadable" || (b.why === "unreadable" && a.error === b.error);
}

const NO_TOOLS = "so this reply comes straight from the local model with no tools: it cannot read your node, wallet, memory or files, or change anything.";

/** HUP-S1.1 notice, kept word for word: the status read says Hermes is not running. */
export const SIDECAR_DOWN_NOTICE = `Hermes is not running right now, ${NO_TOOLS}`;

/** The member switched the sidecar loop off; Hermes may well be running. */
export const SIDECAR_LOOP_OFF_NOTICE = `Hermes's agent loop is switched off in Settings (Run Hermes's loop in the agent sidecar), ${NO_TOOLS}`;

/** The status call failed: Hermes's state is not known, and the notice says so instead of guessing. */
export function sidecarUnreadableNotice(error: string): string {
  return `Hermes did not answer a status check (${error}), ${NO_TOOLS}`;
}

/** The toast for a reason. */
export function noticeFor(reason: SidecarDownReason): string {
  if (reason.why === "loop_off") return SIDECAR_LOOP_OFF_NOTICE;
  if (reason.why === "unreadable") return sidecarUnreadableNotice(reason.error);
  return SIDECAR_DOWN_NOTICE;
}

/** The chat header label for a reason. */
export function labelFor(reason: SidecarDownReason): string {
  if (reason.why === "loop_off") return "local model · llama-server · Hermes loop off (no tools)";
  if (reason.why === "unreadable") return "local model · llama-server · Hermes status unknown (no tools)";
  return "local model · llama-server · Hermes offline (no tools)";
}

/** Whether the Agent slice's Hermes row reads running. */
export function hermesRowRunning(attached: readonly { id: string; state: string }[]): boolean {
  return attached.some((r) => r.id === "hermes" && r.state === "running");
}

/**
 * Call `onFlip` each time the Hermes row flips between running and not running. `subscribe`/`get`
 * are the Agent slice's. Returns the unsubscribe.
 */
export function onHermesFlip(
  subscribe: (fn: () => void) => () => void,
  get: () => { attached: readonly { id: string; state: string }[] },
  onFlip: (running: boolean) => void,
): () => void {
  let was = hermesRowRunning(get().attached);
  return subscribe(() => {
    const now = hermesRowRunning(get().attached);
    if (now === was) return;
    was = now;
    onFlip(now);
  });
}

function errorText(e: unknown): string {
  const s = e instanceof Error ? e.message : typeof e === "string" ? e : String(e);
  return s.length > 160 ? s.slice(0, 157) + "..." : s;
}
