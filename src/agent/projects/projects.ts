// =====================================================================
// HGW-W3 add-on (core #289) — the Projects list (webview half).
//
// `project_create` lives in the Hermes sidecar (citrate-agent-runtime #81, owner decision
// 2026-10-15); core offers no tool of that name. Core keeps the projects folder
// (`~/Citrate/Projects`, created and granted only when the member allows the workspace card), the
// projects Hermes created and the folders the member marked as projects, and sends the live ones
// with every new conversation as `projectRoots` (src-tauri/src/hermes_projects.rs). This module is
// the typed seam the Projects list uses.
//
// The web preview has no sidecar and no folders to work in, so it says so (Rule 1).
// =====================================================================

/** One row of the Projects list (mirrors Rust `ProjectRow`). */
export interface ProjectRow {
  path: string;
  name: string;
  /** `hermes` (created with project_create) or `member` (marked). */
  origin: string;
  /** The folder is still there. */
  exists: boolean;
  /** It goes to Hermes as a project in new conversations. */
  shared: boolean;
}

/** The projects folder (mirrors Rust `WorkspaceView`). */
export interface ProjectsWorkspace {
  root: string;
  exists: boolean;
  granted: boolean;
}

/** The Projects list (mirrors Rust `ProjectsList`). */
export interface ProjectsList {
  workspace: ProjectsWorkspace;
  projects: ProjectRow[];
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

/** What allowing the workspace does. */
export const WORKSPACE_HINT =
  "Hermes gets read and write access to that one folder (and everything in it), so it can create projects there. " +
  "Every new project still asks you first. You can revoke the access any time in Settings > Folder access.";

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

// The Projects list re-reads when core kept a new project or the workspace was allowed from the
// chat's card, so the rail never shows a stale list.
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
