// =====================================================================
// HGW-W3 add-on (core #289) — the Projects list (Agent surface).
//
// Shows the projects folder (`~/Citrate/Projects`) and, until the member allows it, the workspace
// card that creates it and shares it with Hermes for reading and writing. Lists every project
// Hermes created (each one approved on its own card) and every folder the member marked as a
// project, with whether it still goes to Hermes (it exists and is shared for reading and writing).
// Forget takes a project off the list; its files are never touched.
// =====================================================================
import { useCallback, useEffect, useState, type ReactNode } from "react";
import {
  PROJECTS_DESKTOP_ONLY,
  WORKSPACE_HINT,
  WORKSPACE_TITLE,
  errorMessage,
  notifyProjectsChanged,
  onProjectsChanged,
  type ProjectRow,
  type ProjectsIo,
  type ProjectsList,
} from "./projects";

export interface ProjectsPanelProps {
  io: () => Promise<ProjectsIo>;
}

const note = { fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 } as const;

function rowState(r: ProjectRow): string {
  if (!r.exists) return "folder missing";
  if (!r.shared) return "not shared with Hermes";
  return r.origin === "hermes" ? "created by Hermes" : "marked by you";
}

export function ProjectsPanel({ io }: ProjectsPanelProps) {
  const [x, setX] = useState<ProjectsIo | null>(null);
  const [view, setView] = useState<ProjectsList | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [rev, setRev] = useState(0);

  // A project kept from the chat (or the workspace allowed there) re-reads the list.
  useEffect(() => onProjectsChanged(() => setRev((n) => n + 1)), []);

  const read = useCallback(async (got: ProjectsIo) => {
    setView(await got.invoke<ProjectsList>("hermes_projects_list", {}));
  }, []);

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const got = await io();
        if (!live) return;
        setX(got);
        if (got.mode !== "tauri") return;
        const v = await got.invoke<ProjectsList>("hermes_projects_list", {});
        if (live) setView(v);
      } catch (e) {
        if (live) setLoadError(errorMessage(e));
      }
    })();
    return () => {
      live = false;
    };
  }, [io, rev]);

  const run = async (f: () => Promise<ProjectsList>) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      setView(await f());
    } catch (e) {
      setError(errorMessage(e));
      if (x) await read(x).catch(() => undefined);
    } finally {
      setBusy(false);
    }
  };

  const shell = (body: ReactNode) => (
    <div className="surface" data-testid="projects-panel" style={{ display: "flex", flexDirection: "column" }}>
      <div style={{ display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
        <span style={{ fontSize: 13.5, fontWeight: 500 }}>Projects</span>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 10, padding: "12px 16px" }}>{body}</div>
    </div>
  );

  if (x && x.mode !== "tauri") return shell(<span style={note} data-testid="projects-desktop-only">{PROJECTS_DESKTOP_ONLY}</span>);
  if (loadError) return shell(<span style={{ ...note, color: "var(--danger)" }}>Could not read the projects: {loadError}</span>);
  if (!x || !view) return shell(<span style={note}>Reading projects…</span>);

  const mark = async () => {
    let path: string | null;
    try {
      path = await x.pickFolder();
    } catch (e) {
      setError("Could not open the folder picker: " + errorMessage(e));
      return;
    }
    if (!path) return;
    const p = path;
    await run(() => x.invoke<ProjectsList>("hermes_projects_mark", { path: p }));
  };

  return shell(
    <>
      {view.workspace.granted ? (
        <span style={note} data-testid="projects-root">
          Hermes creates new projects in <span className="mono">{view.workspace.root}</span>. Each new project asks you first.
        </span>
      ) : (
        <div data-testid="projects-workspace-card" style={{ display: "flex", flexDirection: "column", gap: 8, border: "1px solid var(--line-2)", borderRadius: "var(--r-1)", padding: 12 }}>
          <span style={{ fontSize: 12.5, fontWeight: 500 }}>{WORKSPACE_TITLE}</span>
          <span style={note}>{WORKSPACE_HINT}</span>
          <span>
            <button
              className="btn btn-primary btn-sm"
              data-testid="projects-workspace-allow"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await x.invoke("hermes_projects_workspace_grant", {});
                  notifyProjectsChanged();
                  return x.invoke<ProjectsList>("hermes_projects_list", {});
                })
              }
            >
              Allow
            </button>
          </span>
        </div>
      )}
      {view.projects.length === 0 ? (
        <span style={note}>No projects yet. Ask Hermes to create one, or mark a folder you already shared with Hermes.</span>
      ) : (
        <div style={{ display: "flex", flexDirection: "column" }}>
          {view.projects.map((r) => (
            <div key={r.path} data-testid="project-row" style={{ display: "flex", gap: 10, alignItems: "center", borderBottom: "1px solid var(--line-1)", padding: "7px 0", opacity: r.exists && r.shared ? 1 : 0.6 }}>
              <span style={{ flex: 1, minWidth: 0 }}>
                <span className="mono" style={{ display: "block", fontSize: 12, fontWeight: 500 }}>{r.name}</span>
                <span className="mono" style={{ display: "block", fontSize: 10, color: "var(--tx-3)", wordBreak: "break-all" }}>{r.path}</span>
                <span style={{ display: "block", fontSize: 10.5, color: r.exists && r.shared ? "var(--tx-2)" : "var(--warn)" }}>{rowState(r)}</span>
              </span>
              <button className="btn btn-ghost btn-sm" data-testid="project-forget" disabled={busy} title="Take it off the list. Its files stay." onClick={() => void run(() => x.invoke<ProjectsList>("hermes_projects_forget", { path: r.path }))}>
                Forget
              </button>
            </div>
          ))}
        </div>
      )}
      <span style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
        <button className="btn btn-ghost btn-sm" data-testid="projects-mark" disabled={busy} onClick={() => void mark()}>
          Mark a folder as a project
        </button>
        <span style={note}>A marked folder must be shared with Hermes for reading and writing (Settings &gt; Folder access).</span>
      </span>
      {error && (
        <span role="alert" data-testid="projects-error" style={{ fontSize: 12, color: "var(--danger)" }}>
          {error}
        </span>
      )}
    </>,
  );
}
