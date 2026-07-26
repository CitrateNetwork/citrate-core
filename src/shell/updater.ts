// =====================================================================
// citrate-core — in-app auto-update (W2.4)
//
// Drives the `tauri-plugin-updater` check → download → install → relaunch
// flow from a signed GitHub Releases feed (latest.json; pubkey pinned in
// tauri.conf.json). Honest by construction (Rule 1):
//   - Only ever runs in a packaged Tauri build. In sim/web the whole flow is a
//     no-op ("idle") — no fabricated "update available", no fake progress.
//   - Progress bytes are the REAL bytes the plugin reports, not a synthetic bar.
//   - The app is never yanked out from under the user: install stages the
//     update, and the RESTART is always an explicit click (even for a
//     critical patch, which only auto-*downloads*).
//
// The plugin modules are dynamically imported inside the Tauri-gated path so the
// web bundle never eagerly loads Tauri internals.
// =====================================================================
import { useCallback, useEffect, useRef, useState } from "react";
import { BRIDGE_MODE } from "../bridge/mode";

/** How often to re-check for an update while the app is open (6 h). */
export const UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

export type UpdateStatus =
  | "idle" // not checked yet / not applicable (sim)
  | "checking"
  | "uptodate"
  | "available"
  | "downloading"
  | "ready" // downloaded + staged; awaiting an explicit restart
  | "error";

export interface UpdateState {
  status: UpdateStatus;
  /** The available/target version (e.g. "0.2.0"), when known. */
  version: string | null;
  /** Release notes / body from latest.json, when present. */
  notes: string | null;
  /** A critical patch auto-downloads (still an explicit restart). */
  critical: boolean;
  downloadedBytes: number;
  totalBytes: number | null;
  error: string | null;
  /** Dismissed by the user for this session (banner hidden until next find). */
  dismissed: boolean;
}

export const INITIAL_UPDATE_STATE: UpdateState = {
  status: "idle",
  version: null,
  notes: null,
  critical: false,
  downloadedBytes: 0,
  totalBytes: null,
  error: null,
  dismissed: false,
};

/** A release is "critical" when its notes carry an explicit marker. Kept simple
 *  and case-insensitive so the CI/release author opts in per-release. */
export function isCriticalNotes(notes: string | null | undefined): boolean {
  if (!notes) return false;
  return /\[critical\]|^\s*critical:/i.test(notes);
}

/** Human byte size for honest progress display. */
export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u += 1;
  }
  // One decimal below 10 (except bytes), integer otherwise; trailing ".0" dropped.
  const rounded = u === 0 || v >= 10 ? Math.round(v) : Math.round(v * 10) / 10;
  return `${rounded} ${units[u]}`;
}

/** Download progress as a 0..1 fraction, or null when the total is unknown. */
export function progressFraction(s: Pick<UpdateState, "downloadedBytes" | "totalBytes">): number | null {
  if (!s.totalBytes || s.totalBytes <= 0) return null;
  return Math.min(1, s.downloadedBytes / s.totalBytes);
}

// The live plugin handle for the pending update (set between check and install).
// Kept module-local (not React state) since it is not serializable.
type PluginUpdate = {
  version: string;
  body?: string | null;
  downloadAndInstall: (
    onEvent: (e: { event: "Started" | "Progress" | "Finished"; data?: { contentLength?: number; chunkLength?: number } }) => void,
  ) => Promise<void>;
};

/**
 * React hook: auto-check on mount (Tauri only) + every {@link UPDATE_CHECK_INTERVAL_MS},
 * expose install/dismiss/recheck. In sim/web the hook stays "idle" forever.
 */
export function useAppUpdate() {
  const [state, setState] = useState<UpdateState>(INITIAL_UPDATE_STATE);
  const pending = useRef<PluginUpdate | null>(null);
  const patch = useCallback((p: Partial<UpdateState>) => setState((s) => ({ ...s, ...p })), []);

  const check = useCallback(async () => {
    if (BRIDGE_MODE !== "tauri") return; // honest no-op off-desktop
    // Don't clobber an in-flight download/ready with a re-check.
    setState((s) => (s.status === "downloading" || s.status === "ready" ? s : { ...s, status: "checking", error: null }));
    try {
      const { check: pluginCheck } = await import("@tauri-apps/plugin-updater");
      const update = (await pluginCheck()) as PluginUpdate | null;
      if (!update) {
        setState((s) => (s.status === "downloading" || s.status === "ready" ? s : { ...s, status: "uptodate" }));
        return;
      }
      pending.current = update;
      const critical = isCriticalNotes(update.body);
      setState((s) => ({
        ...s,
        status: "available",
        version: update.version,
        notes: update.body ?? null,
        critical,
        dismissed: false,
      }));
    } catch (e) {
      patch({ status: "error", error: e instanceof Error ? e.message : String(e) });
    }
  }, [patch]);

  const install = useCallback(async () => {
    const update = pending.current;
    if (!update || BRIDGE_MODE !== "tauri") return;
    patch({ status: "downloading", downloadedBytes: 0, totalBytes: null, error: null });
    try {
      let downloaded = 0;
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") {
          patch({ totalBytes: e.data?.contentLength ?? null });
        } else if (e.event === "Progress") {
          downloaded += e.data?.chunkLength ?? 0;
          setState((s) => ({ ...s, downloadedBytes: downloaded }));
        } else if (e.event === "Finished") {
          setState((s) => ({ ...s, status: "ready" }));
        }
      });
      patch({ status: "ready" });
    } catch (e) {
      patch({ status: "error", error: e instanceof Error ? e.message : String(e) });
    }
  }, [patch]);

  const restart = useCallback(async () => {
    if (BRIDGE_MODE !== "tauri") return;
    const { relaunch } = await import("@tauri-apps/plugin-process");
    await relaunch();
  }, []);

  const dismiss = useCallback(() => patch({ dismissed: true }), [patch]);

  // Auto-check on mount + interval (Tauri only).
  useEffect(() => {
    if (BRIDGE_MODE !== "tauri") return;
    void check();
    const id = window.setInterval(() => void check(), UPDATE_CHECK_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [check]);

  // A critical update auto-starts the DOWNLOAD (never the restart).
  useEffect(() => {
    if (state.status === "available" && state.critical) void install();
  }, [state.status, state.critical, install]);

  return { state, check, install, restart, dismiss };
}
