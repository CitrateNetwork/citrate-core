// =====================================================================
// Hermes terminal access: the member's switch for the sidecar's `shell_run` tool, TypeScript side.
//
// Core (src-tauri/src/hermes_terminal.rs) stores the choice and sets CITRATE_HERMES_SHELL_RUN for
// the Hermes sidecar when it starts (pinned empty when off). Changing it restarts a running
// sidecar so the next conversation has (or no longer has) the tool. The tool itself lives in the
// sidecar (citrate-agent-runtime agent-sidecar shell_run.rs): offered only to conversations with a
// shared folder, every command held for the member's HIC decision on an approval card, run inside
// the OS sandbox (refused when there is none). On by default (owner decision 2026-10-04).
// =====================================================================

/** The setting's label in Settings. */
export const TERMINAL_ACCESS_LABEL = "Let Hermes run terminal commands";

/** The plain-words explanation under the switch. No em-dashes (house style). */
export const TERMINAL_ACCESS_HINT =
  "On by default. Every command asks you first, and you see the exact command and folder before it runs. " +
  "Commands run in a sandbox with no network, and only in folders you've shared with Hermes (Folder access, below). " +
  "Turning this off or on restarts Hermes so the change applies to new conversations.";

/** What `hermes_terminal_set` / `hermes_terminal_get` return (camelCase, mirrors the Rust struct). */
export interface TerminalAccessStatus {
  enabled: boolean;
  /** True when a running Hermes was restarted to apply the change. */
  restarted: boolean;
  /** A note when the saved setting could not be read (it then counts as off). */
  loadError?: string | null;
}

export interface TerminalAccessIo {
  mode: "tauri" | "sim";
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
}

/** Send the member's choice to core. In the web preview there is no sidecar: nothing is sent and
 *  nothing restarts. */
export async function syncTerminalAccess(io: TerminalAccessIo, enabled: boolean): Promise<TerminalAccessStatus> {
  if (io.mode !== "tauri") return { enabled, restarted: false };
  return io.invoke<TerminalAccessStatus>("hermes_terminal_set", { enabled });
}
