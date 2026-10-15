// =====================================================================
// HGW-W3 (core #289) — Hermes's projects (webview half).
//
// The tool that creates a project, `project_create`, lives in the Hermes sidecar
// (citrate-agent-runtime #81, owner decision 2026-10-15); core offers no tool of that name. Core
// keeps the projects folder (`~/Citrate/Projects`, created and granted only when the member allows
// it on the workspace card) and the list of projects (the ones Hermes created, and folders the
// member marked), and sends the live ones with every new Hermes conversation as `projectRoots`
// (src-tauri/src/hermes_projects.rs). This module is the typed seam to those commands and the
// parsing of a `project_create` result.
//
// The web preview has no sidecar and no folders to work in, so it says so (Rule 1).
// =====================================================================

/** The sidecar tool that creates a project (runtime-hosted). */
export const PROJECT_CREATE_TOOL = "project_create";

/** One row of the Projects list (mirrors Rust `ProjectRow`). */
export interface ProjectRow {
  path: string;
  name: string;
  origin: "hermes" | "member";
  addedAt: number;
  /** The folder is still there. */
  exists: boolean;
  /** Granted for reading and writing, so Hermes gets it as a project in new conversations. */
  shared: boolean;
}

/** The Projects list (mirrors Rust `ProjectsView`). */
export interface ProjectsView {
  root: string;
  rootExists: boolean;
  workspaceReady: boolean;
  projects: ProjectRow[];
  loadError: string | null;
}

/** The IO seam: the runtime, the native folder picker and the Rust commands. */
export interface ProjectsIo {
  mode: "tauri" | "sim";
  pickFolder(): Promise<string | null>;
  invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T>;
}

export const PROJECTS_DESKTOP_ONLY = "Projects need the desktop app. The web preview has no Hermes to build them.";

/** The workspace card's question (no em-dashes, house style). */
export const WORKSPACE_TITLE = "Let Hermes create and work in ~/Citrate/Projects";

/** What allowing the workspace does, shown on the card and in the Projects list. */
export const WORKSPACE_HINT =
  "Hermes gets read and write access to that one folder (and everything in it), so it can create projects there. " +
  "Every new project still asks you first. You can revoke the access any time in Settings > Folder access.";

/** Why the member is asked (the HIC banner on the workspace card). */
export const WORKSPACE_HIC_REASON =
  "Hermes asked to create a project, and its projects folder is not set up yet. Allowing this creates ~/Citrate/Projects and shares it with Hermes for reading and writing.";

/**
 * A `project_create` result's project, from the sidecar's `tool_result` content: the JSON report
 * `{ok, name, path, ...}`. Null for anything else (an error, a refusal, malformed text).
 */
export function projectCreatedOf(status: string, content: string): { name: string; path: string } | null {
  if (status !== "ok") return null;
  let v: unknown;
  try {
    v = JSON.parse(content);
  } catch {
    return null;
  }
  if (typeof v !== "object" || v === null) return null;
  const r = v as Record<string, unknown>;
  if (r.ok !== true || typeof r.path !== "string" || !r.path.startsWith("/") || typeof r.name !== "string") return null;
  return { name: r.name, path: r.path };
}

/**
 * Whether a `project_create` result failed because the projects folder is missing or not shared
 * with Hermes (the runtime's own wording), so the member should be offered the workspace card.
 */
export function needsWorkspace(status: string, content: string): boolean {
  if (status === "ok") return false;
  return /the projects folder .* (does not exist yet|is not granted for reading and writing)/.test(content);
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** The production IO: the bridge's runtime mode, the Tauri dialog plugin and the timeout-wrapped invoke. */
export async function desktopProjectsIo(): Promise<ProjectsIo> {
  const { BRIDGE_MODE } = await import("../../bridge/mode");
  return {
    mode: BRIDGE_MODE,
    pickFolder: async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ directory: true, multiple: false, title: "Choose a folder to use as a project" });
      return typeof picked === "string" ? picked : null;
    },
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      const { invoke } = await import("../../bridge/tauri/invoke");
      return invoke<T>(cmd, args);
    },
  };
}

/** Allow the workspace (create `~/Citrate/Projects`, share it for reading and writing). */
export function enableWorkspace(io: ProjectsIo): Promise<ProjectsView> {
  return io.invoke<ProjectsView>("hermes_projects_workspace_enable", {});
}

/** Keep a project Hermes created (core checks it is in the projects folder and has its marker). */
export function recordCreated(io: ProjectsIo, path: string): Promise<ProjectsView> {
  return io.invoke<ProjectsView>("hermes_projects_record_created", { path });
}

// The Projects list re-reads when core kept a new project or the workspace was allowed elsewhere
// (the chat's workspace card), so the rail never shows a stale list.
const listeners = new Set<() => void>();

/** Subscribe to project list changes; returns the unsubscribe. */
export function onProjectsChanged(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Tell every open Projects list to re-read. */
export function notifyProjectsChanged(): void {
  for (const cb of [...listeners]) cb();
}
