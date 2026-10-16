// =====================================================================
// HUP-S2.1 — Hermes folder grants (webview half).
//
// The grant document lives in Rust (src-tauri/src/agent_grants.rs), in core's
// app data, which the agent can never read or write. This module is the typed
// seam to those commands plus the countdown formatting the Grants panel shows.
// Every change is sent by Rust to each open Hermes conversation, and the agent
// sidecar checks every file path against it at the moment of use.
//
// The web preview has no grant store and no agent, so it says so (Rule 1).
// =====================================================================

/** One row of the Grants list (mirrors Rust `GrantRow`). */
export interface GrantRow {
  id: string;
  kind: "folder" | "full_access" | string;
  root: string;
  access: "read" | "write";
  /** `blocked`: rooted in a protected location, so Hermes ignores it (it can still be revoked). */
  status: "active" | "expired" | "revoked" | "not_yet_active" | "blocked" | string;
  grantedAt: number;
  expiresAt: number | null;
  remainingSecs: number | null;
  reason: string;
}

/** The Grants panel view (mirrors Rust `GrantsView`). */
export interface GrantsView {
  status: "ok" | "corrupted" | string;
  error: string | null;
  grants: GrantRow[];
  fullAccessRemainingSecs: number | null;
  fullAccessRoot: string;
  now: number;
}

/** How sending a change to the open Hermes conversations went (mirrors Rust `GrantsPushOutcome`). */
export interface GrantsSync {
  updated: number;
  failed: string[];
}

export interface GrantsChange {
  view: GrantsView;
  sync: GrantsSync;
}

/** Step 1 of the full-access HIC-1 confirmation (mirrors Rust `FullAccessConfirmation`). */
export interface FullAccessConfirmation {
  id: string;
  root: string;
  statement: string;
  preparedAt: number;
  confirmBy: number;
  grantExpiresAt: number;
}

/** The IO seam: the runtime, the native folder picker and the Rust commands. */
export interface GrantsIo {
  mode: "tauri" | "sim";
  pickFolder(): Promise<string | null>;
  invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T>;
}

export const DESKTOP_ONLY = "Folder grants need the desktop app. The web preview has no agent to grant folders to.";

/** A countdown like "23 h 59 m", "4 m 05 s" or "12 s". Never rounds up; negative is "0 s". */
export function fmtCountdown(secs: number): string {
  const s = Math.max(0, Math.floor(secs));
  const pad = (n: number) => String(n).padStart(2, "0");
  if (s >= 3600) return `${Math.floor(s / 3600)} h ${pad(Math.floor((s % 3600) / 60))} m`;
  if (s >= 60) return `${Math.floor(s / 60)} m ${pad(s % 60)} s`;
  return `${s} s`;
}

/** The member-facing line for a sync result, or null when there is nothing to say. */
export function syncMessage(sync: GrantsSync): string | null {
  const parts: string[] = [];
  if (sync.updated > 0) parts.push(`Sent to ${sync.updated} open Hermes conversation${sync.updated === 1 ? "" : "s"}.`);
  for (const f of sync.failed) parts.push(f.charAt(0).toUpperCase() + f.slice(1) + ".");
  return parts.length ? parts.join(" ") : null;
}

export function errorMessage(e: unknown): string {
  if (e instanceof Error) return e.message;
  return String(e);
}

/** The production IO: the bridge's runtime mode, the Tauri dialog plugin and the timeout-wrapped invoke. */
export async function desktopGrantsIo(): Promise<GrantsIo> {
  const { BRIDGE_MODE } = await import("../../bridge/mode");
  return {
    mode: BRIDGE_MODE,
    pickFolder: async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ directory: true, multiple: false, title: "Choose a folder for Hermes" });
      return typeof picked === "string" ? picked : null;
    },
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      const { invoke } = await import("../../bridge/tauri/invoke");
      return invoke<T>(cmd, args);
    },
  };
}
