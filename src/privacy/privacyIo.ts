// =====================================================================
// HUP-S10.5 — the IO seam for the privacy and recovery panel.
//
// The runtime mode, the native save/open dialogs and the Rust commands, behind
// one small interface so the logic and the panel are tested without Tauri.
// The web preview has no keychain and no files, so every action there says so
// instead of pretending (Rule 1).
// =====================================================================

export interface DialogFilter {
  name: string;
  extensions: string[];
}

export interface PrivacyIo {
  mode: "tauri" | "sim";
  pickSavePath(defaultName: string, filter: DialogFilter): Promise<string | null>;
  pickOpenPath(filter: DialogFilter): Promise<string | null>;
  invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T>;
  /** Clear this app's webview storage (local and session storage). */
  clearWebStorage(): void;
}

export const DESKTOP_ONLY = "This needs the desktop app. The web preview has no keychain and no local files.";

/** Deadlines for the commands that legitimately take longer than the default 12 s. */
const DEADLINES_MS: Record<string, number> = {
  recovery_kit_save_file: 60_000, // Argon2id
  recovery_kit_restore_file: 60_000,
  local_data_plan: 120_000, // sizes a large models folder
  local_data_delete: Infinity, // stops every sidecar, then deletes gigabytes
};

export function messageOf(e: unknown): string {
  if (e instanceof Error) return e.message;
  return String(e);
}

/** The production IO: bridge mode, the Tauri dialog plugin and the timeout-wrapped invoke. */
export async function desktopPrivacyIo(): Promise<PrivacyIo> {
  const { BRIDGE_MODE } = await import("../bridge/mode");
  return {
    mode: BRIDGE_MODE,
    pickSavePath: async (defaultPath, filter) => {
      const { save } = await import("@tauri-apps/plugin-dialog");
      return save({ defaultPath, filters: [filter] });
    },
    pickOpenPath: async (filter) => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ multiple: false, directory: false, filters: [filter] });
      return typeof picked === "string" ? picked : null;
    },
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      const { invoke } = await import("../bridge/tauri/invoke");
      return invoke<T>(cmd, args, DEADLINES_MS[cmd]);
    },
    clearWebStorage: () => {
      try {
        window.localStorage.clear();
        window.sessionStorage.clear();
      } catch {
        /* storage may be unavailable; the folder deletion still removes it on disk */
      }
    },
  };
}
