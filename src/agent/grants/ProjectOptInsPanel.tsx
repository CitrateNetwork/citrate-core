// =====================================================================
// V1 (owner decision 2026-11-16): per-project opt-ins for Hermes terminal commands
// (Hermes › Controls › Details › Terminal & folders).
//
// Inside a project, the Hermes sidecar runs a short list of read and build commands without a
// card. Three of them run code the project defines or make a commit, so they ask by default:
// `node <file>`, `npm run …` and `git commit`. Here the member turns any of the three on for one
// project folder at a time. The choice is stored by core (src-tauri/src/hermes_auto_approve.rs),
// sent with every new conversation, and enforced by the sidecar. Hermes has no tool that reaches
// it.
//
// Data sources (Rule 7): the rules come from the sidecar's list in effect
// (`hermes_auto_approve_rules`, `GET /shell/auto-approve`); a sidecar that marks no rule
// `opt_in` offers nothing to turn on, and the panel says so. The saved choice comes from
// `hermes_auto_approve_get`.
// =====================================================================
import { useCallback, useEffect, useState } from "react";
import { DESKTOP_ONLY, errorMessage, type GrantsIo } from "./grants";

/** One saved project (mirrors Rust `ProjectOptIn`). */
export interface ProjectOptIn {
  root: string;
  rules: string[];
  changedAtMs: number;
}

/** What `hermes_auto_approve_get` / `_set` return (mirrors Rust `OptInsView`). */
export interface OptInsView {
  projects: ProjectOptIn[];
  loadError: string | null;
}

/** One rule of the sidecar's list, as `GET /shell/auto-approve` returns it. */
interface ListedRule {
  id: string;
  opt_in?: boolean;
}

/** The plain words for each opt-in rule the shipped list has. An id the app does not know shows
 *  as its id. */
export const OPT_IN_LABELS: Record<string, string> = {
  "node-script": "node <file>",
  "npm-run": "npm run build, test, lint or typecheck",
  "npm-test": "npm test",
  "npx-tsc": "npx tsc --noEmit",
  "forge-build-test": "forge build or test",
  "cargo-offline": "cargo build, test or check --offline",
  "git-status": "git status",
  "git-diff": "git diff",
  "git-add": "git add",
  "git-commit": "git commit",
};

export const OPT_INS_HINT =
  "These ask you first by default, because they can run code the project itself defines (scripts, build files, test binaries, git filters) or make a commit. " +
  "Turn one on for a project and Hermes runs it there without asking, in the sandbox with no network, writing only that project. " +
  "It asks again once the conversation reads something from outside (a web page, a tool's output or a group message). Changes apply to new conversations.";

export interface ProjectOptInsPanelProps {
  io: () => Promise<GrantsIo>;
}

type RulesState =
  | { kind: "loading" }
  | { kind: "stopped" }
  | { kind: "none"; defaults: string[] }
  | { kind: "ok"; ids: string[]; defaults: string[] }
  | { kind: "error"; reason: string };

export function ProjectOptInsPanel({ io }: ProjectOptInsPanelProps) {
  const [x, setX] = useState<GrantsIo | null>(null);
  const [rules, setRules] = useState<RulesState>({ kind: "loading" });
  const [view, setView] = useState<OptInsView | null>(null);
  // A folder the member picked but has not turned anything on for yet (nothing is saved for it).
  const [draftRoot, setDraftRoot] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const got = await io();
        if (!live) return;
        setX(got);
        if (got.mode !== "tauri") return;
        const [list, saved] = await Promise.all([
          got.invoke<{ rules?: ListedRule[] } | null>("hermes_auto_approve_rules", {}).catch((e) => new Error(errorMessage(e))),
          got.invoke<OptInsView>("hermes_auto_approve_get", {}),
        ]);
        if (!live) return;
        setView(saved);
        if (list instanceof Error) setRules({ kind: "error", reason: list.message });
        else if (list === null) setRules({ kind: "stopped" });
        else {
          const ids = (list.rules ?? []).filter((r) => r.opt_in === true).map((r) => r.id);
          // What this build runs without a card in any project, read from the sidecar itself.
          const defaults = list.enabled === false ? [] : (list.rules ?? []).filter((r) => r.opt_in !== true).map((r) => r.id);
          setRules(ids.length ? { kind: "ok", ids, defaults } : { kind: "none", defaults });
        }
      } catch (e) {
        if (live) setError(errorMessage(e));
      }
    })();
    return () => {
      live = false;
    };
  }, [io]);

  const save = useCallback(
    async (root: string, next: string[]) => {
      if (!x) return;
      setBusy(true);
      setError(null);
      try {
        const v = await x.invoke<OptInsView>("hermes_auto_approve_set", { root, rules: next });
        setView(v);
        if (draftRoot === root) setDraftRoot(null);
      } catch (e) {
        setError(errorMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [x, draftRoot],
  );

  if (x && x.mode !== "tauri") {
    return <p style={{ fontSize: 12, color: "var(--tx-3)", margin: 0 }}>{DESKTOP_ONLY}</p>;
  }

  const ids = rules.kind === "ok" ? rules.ids : [];
  const saved = view?.projects ?? [];
  const rows: ProjectOptIn[] = draftRoot && !saved.some((p) => p.root === draftRoot) ? [...saved, { root: draftRoot, rules: [], changedAtMs: 0 }] : saved;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }} data-testid="project-opt-ins">
      <span style={{ fontSize: 13, fontWeight: 500 }}>Commands Hermes may run without asking, per project</span>
      <span style={{ fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 }}>{OPT_INS_HINT}</span>
      {rules.kind === "stopped" && <p role="status" style={{ fontSize: 12, margin: 0 }}>Start Hermes to see which commands this build lets you turn on.</p>}
      {(rules.kind === "ok" || rules.kind === "none") && (
        <p data-testid="auto-defaults" style={{ fontSize: 12, margin: 0 }}>
          {rules.defaults.length
            ? `In any project, this Hermes build runs these without asking: ${rules.defaults.join(", ")}.`
            : "In any project, this Hermes build runs no command without asking."}
        </p>
      )}
      {rules.kind === "none" && <p role="status" style={{ fontSize: 12, margin: 0 }}>This Hermes build has no commands to turn on per project.</p>}
      {rules.kind === "error" && <p role="alert" style={{ fontSize: 12, margin: 0 }}>Could not read the command list from Hermes: {rules.reason}</p>}
      {view?.loadError && <p role="alert" style={{ fontSize: 12, margin: 0 }}>{view.loadError}</p>}
      {rows.length === 0 && rules.kind === "ok" && <p style={{ fontSize: 12, color: "var(--tx-3)", margin: 0 }}>No project has any of these turned on.</p>}
      {rows.map((p) => (
        <fieldset key={p.root} style={{ border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: "8px 12px", margin: 0 }}>
          <legend className="mono" style={{ fontSize: 11 }}>{p.root}</legend>
          {(ids.length ? ids : p.rules).map((id) => (
            <label key={id} style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12.5 }}>
              <input
                type="checkbox"
                checked={p.rules.includes(id)}
                disabled={busy || !ids.includes(id)}
                onChange={(e) => void save(p.root, e.target.checked ? [...p.rules, id] : p.rules.filter((r) => r !== id))}
              />
              <span className="mono">{OPT_IN_LABELS[id] ?? id}</span>
            </label>
          ))}
          {p.rules.length > 0 && (
            <button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => void save(p.root, [])}>
              Turn all off for this project
            </button>
          )}
        </fieldset>
      ))}
      {rules.kind === "ok" && (
        <div>
          <button
            type="button"
            className="btn btn-secondary btn-sm"
            disabled={busy || !x}
            onClick={() =>
              void (async () => {
                const picked = await x?.pickFolder();
                if (picked) setDraftRoot(picked);
              })()
            }
          >
            Choose a project folder
          </button>
        </div>
      )}
      {error && <p role="alert" style={{ fontSize: 12, margin: 0 }}>{error}</p>}
    </div>
  );
}
