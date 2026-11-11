// =====================================================================
// citrate-core — in-app auto-update (W2.4)
//
// Drives the `tauri-plugin-updater` check → download → install → relaunch
// flow from a signed GitHub Releases feed (latest.json; pubkey pinned in
// tauri.conf.json). Honest by construction (Rule 1):
//   - Only ever runs in a packaged Tauri build. In sim/web the whole flow is a
//     no-op ("idle") — no fabricated "update available", no fake progress.
//   - Progress bytes are the REAL bytes the plugin reports, not a synthetic bar.
//   - The app is never yanked out from under the user: the download stages the
//     update, and the RESTART is always an explicit click (even for a
//     critical patch, which only auto-*downloads*).
//   - CORE-D6 (0.5.3): the update is installed only after the sidecars are
//     drained. On the restart click: `update_prepare` (Rust update_drain.rs)
//     holds every new sidecar start, stops every sidecar through its owner and
//     waits for their children to exit; only when it reports every one stopped
//     does `install()` run (bounded by INSTALL_TIMEOUT_MS), then the relaunch.
//     If the drain is incomplete or the install fails, the hold is released
//     (`update_prepare_cancel`) and the member is told why. If the install
//     times out or the relaunch fails, the bundle may already be replaced: the
//     hold stays and the banner asks the member to quit and reopen the app.
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
  | "installing" // CORE-D6: services drained, the update being installed, then the relaunch
  | "installed" // #614 review: installed (or maybe installed) but not relaunched; quit and reopen
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
  download: (
    onEvent: (e: { event: "Started" | "Progress" | "Finished"; data?: { contentLength?: number; chunkLength?: number } }) => void,
  ) => Promise<void>;
  install: () => Promise<void>;
};

/** CORE-D6: what `update_prepare` reports. Mirrors Rust `update_drain::DrainReport`. */
export interface DrainReport {
  allStopped: boolean;
  /** #614 review: sidecars whose child had not exited when the drain stopped waiting. */
  stillExiting?: string[];
  services: { id: string; label: string; before: string; after: string; stopped: boolean }[];
  tookMs: number;
}

/** #619: the code Rust's refusal of a plain cancel starts with once the install began
 *  (update_drain INSTALL_HELD_CODE). Matched by code, never by its wording. */
const INSTALL_HELD_CODE = "install_held";

/** Whether an error from `update_prepare_cancel` is the install-held refusal. */
export function isInstallHeld(message: string): boolean {
  return new RegExp(`(^|:\\s*)${INSTALL_HELD_CODE}:`).test(message);
}

/** #614 review: how long the plugin's install may take before the member is told what to do. */
export const INSTALL_TIMEOUT_MS = 5 * 60 * 1000;

/** The result of applying a downloaded update. `keepStopped`: the bundle may already be replaced,
 *  so the services stay held and the member is asked to quit and reopen the app. */
export type ApplyResult = { ok: true } | { ok: false; error: string; keepStopped?: boolean };

/** CORE-D6: the steps of applying a downloaded update, injected so the order is testable. */
export interface ApplySteps {
  /** Hold new sidecar starts and stop every sidecar (`update_prepare`). */
  drain: () => Promise<DrainReport>;
  /** Release the hold (`update_prepare_cancel`); the sidecars stay stopped. `installFailed`: the
   *  install itself failed (the only release Rust accepts once the install began). */
  cancel: (installFailed?: boolean) => Promise<void>;
  /** #617 review: mark the install begun (`update_install_begin`); refused unless drained. From
   *  then on a reloaded page cannot release the hold. */
  beginInstall: () => Promise<void>;
  /** The plugin's install of the downloaded bytes (on Windows it starts the installer and exits). */
  install: () => Promise<void>;
  relaunch: () => Promise<void>;
}

const msgOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * CORE-D6: drain, then install, then relaunch. The update is installed only when the drain
 * reports every sidecar stopped. Returns an error for the member on any refusal or failure,
 * after releasing the hold.
 */
export async function applyStagedUpdate(steps: ApplySteps, opts: { installTimeoutMs?: number } = {}): Promise<ApplyResult> {
  let report: DrainReport;
  try {
    report = await steps.drain();
  } catch (e) {
    await steps.cancel().catch(() => undefined);
    return { ok: false, error: `The update was not installed: the services could not be stopped (${msgOf(e)}).` };
  }
  if (!report.allStopped) {
    const left = report.services.filter((s) => !s.stopped).map((s) => `${s.label} (${s.after})`);
    const exiting = (report.stillExiting ?? []).map((n) => `${n} is still exiting`);
    await steps.cancel().catch(() => undefined);
    return {
      ok: false,
      error: `The update was not installed: ${[...left.map((l) => `${l} did not stop`), ...exiting].join(", ") || "a service did not stop"}. Services stay stopped until you start them or restart the app.`,
    };
  }
  try {
    await steps.beginInstall();
  } catch (e) {
    await steps.cancel().catch(() => undefined);
    return { ok: false, error: `The update was not installed: ${msgOf(e)}. Services stay stopped until you start them or restart the app.` };
  }
  const limit = opts.installTimeoutMs ?? INSTALL_TIMEOUT_MS;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timedOut = new Promise<"timeout">((res) => {
    timer = setTimeout(() => res("timeout"), limit);
  });
  try {
    const done = await Promise.race([steps.install().then(() => "done" as const), timedOut]);
    if (done === "timeout") {
      // The bundle may be part-way replaced: never release the hold or restart sidecars from it.
      return {
        ok: false,
        keepStopped: true,
        error: `The update did not finish installing within ${Math.round(limit / 60000) || 1} min. Quit and reopen Citrate Core; if the old version opens, update again.`,
      };
    }
  } catch (e) {
    await steps.cancel(true).catch(() => undefined);
    return { ok: false, error: `The update could not be installed: ${msgOf(e)}. Services stay stopped until you start them or restart the app.` };
  } finally {
    if (timer) clearTimeout(timer);
  }
  try {
    await steps.relaunch();
  } catch (e) {
    return { ok: false, keepStopped: true, error: `The update is installed. Please quit and reopen Citrate Core to finish it (the app could not restart itself: ${msgOf(e)}).` };
  }
  return { ok: true };
}

/**
 * React hook: auto-check on mount (Tauri only) + every {@link UPDATE_CHECK_INTERVAL_MS},
 * expose install/dismiss/recheck. In sim/web the hook stays "idle" forever.
 */
/**
 * True when the updater failed only because the release feed has no entry for
 * this OS/arch (e.g. the unsigned Windows build, which ships no updater
 * artifacts). That means "no updates published for this build", not an error.
 * Network, signature and download errors do not match.
 */
export function isNoPlatformInFeed(msg: string): boolean {
  return /found in the response .?platforms.? object|platform[^.]*not (?:found|available)/i.test(msg);
}

export function useAppUpdate() {
  const [state, setState] = useState<UpdateState>(INITIAL_UPDATE_STATE);
  const pending = useRef<PluginUpdate | null>(null);
  const patch = useCallback((p: Partial<UpdateState>) => setState((s) => ({ ...s, ...p })), []);

  const check = useCallback(async () => {
    if (BRIDGE_MODE !== "tauri") return; // honest no-op off-desktop
    // Don't clobber an in-flight download/ready with a re-check.
    setState((s) => (s.status === "downloading" || s.status === "ready" || s.status === "installing" ? s : { ...s, status: "checking", error: null }));
    try {
      const { check: pluginCheck } = await import("@tauri-apps/plugin-updater");
      const update = (await pluginCheck()) as PluginUpdate | null;
      if (!update) {
        setState((s) => (s.status === "downloading" || s.status === "ready" || s.status === "installing" ? s : { ...s, status: "uptodate" }));
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
      const msg = e instanceof Error ? e.message : String(e);
      // A manifest that doesn't list this OS/arch means updates simply aren't
      // published for this build (e.g. the unsigned Windows build: no updater
      // artifacts, no signing key, no windows-* platform in the feed). That is
      // expected, not a failure the user should see — treat it as up-to-date so
      // the "Update failed" banner never appears. Real errors (network,
      // signature, download) still surface. Mac/Linux are listed, so unaffected.
      if (isNoPlatformInFeed(msg)) {
        setState((s) => (s.status === "downloading" || s.status === "ready" ? s : { ...s, status: "uptodate", error: null }));
        return;
      }
      patch({ status: "error", error: msg });
    }
  }, [patch]);

  const install = useCallback(async () => {
    const update = pending.current;
    if (!update || BRIDGE_MODE !== "tauri") return;
    patch({ status: "downloading", downloadedBytes: 0, totalBytes: null, error: null });
    try {
      let downloaded = 0;
      // CORE-D6: download only; the install runs after the drain, on the restart click.
      await update.download((e) => {
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
    const update = pending.current;
    if (!update || BRIDGE_MODE !== "tauri") return;
    patch({ status: "installing", error: null });
    const { invoke } = await import("../bridge/tauri/invoke");
    const { relaunch } = await import("@tauri-apps/plugin-process");
    const r = await applyStagedUpdate({
      // Each sidecar may take its full stop grace; the drain gets three minutes.
      drain: () => invoke<DrainReport>("update_prepare", {}, 180_000),
      cancel: (installFailed?: boolean) => invoke<void>("update_prepare_cancel", { installFailed: installFailed ?? null }),
      beginInstall: () => invoke<void>("update_install_begin", {}),
      install: () => update.install(),
      relaunch,
    });
    if (!r.ok) patch({ status: r.keepStopped ? "installed" : "error", error: r.error });
  }, [patch]);

  const dismiss = useCallback(() => patch({ dismissed: true }), [patch]);

  // A fresh page has no install in flight: release a hold an earlier page left (it reloaded
  // between the drain and the install or cancel). Nothing is restarted.
  useEffect(() => {
    if (BRIDGE_MODE !== "tauri") return;
    // #614 review (P3): an earlier page's drain must not hold the services for the session.
    // #617 review: once an install began Rust refuses this (INSTALL_HELD_CODE): the hold stays until
    // the app quits, and this page shows the installed state instead of offering a restart.
    void import("../bridge/tauri/invoke")
      .then(({ invoke }) => invoke<void>("update_prepare_cancel", {}))
      .catch((e: unknown) => {
        const m = msgOf(e);
        if (isInstallHeld(m)) patch({ status: "installed", error: "The update is installed. Please quit and reopen Citrate Core to finish it." });
      });
  }, [patch]);

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
