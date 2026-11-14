// =====================================================================
// 0.5.4 (owner, 2026-11-14): Hermes is complete on install, and anything missing is said in the UI.
//
// Core provisions Hermes at every launch (src-tauri/src/hermes_setup.rs: data folders, the pinned
// starter capsules, first-run defaults for the node MCP tools) and `hermes_setup_check` reports
// what it found. This slice adds what only the app can check, and runs the whole self-check at
// launch and each time Hermes starts or stops:
//   - the tools sent to every Hermes session (each has its effect/trust annotation, which
//     build_session_body requires) and the system prompt are present;
//   - while Hermes runs, it reports the skills it loaded (GET /status through hermes_status).
// A read that fails is an honest error row, never a pass.
// =====================================================================
import { createSlice } from "./createSlice";
import { invoke } from "../../bridge/tauri/invoke";
import { bridge } from "../../bridge";
import { BRIDGE_MODE } from "../../bridge/mode";
import { AGENT_SYSTEM_PROMPT } from "../../agent/harness";
import { annotatedAgentTools } from "../../agent/toolAnnotations";

export type SetupNeed = "required" | "optional";
/** `unchecked`: can only be checked while Hermes runs (never counts as complete). */
export type SetupState = "ok" | "missing" | "off" | "unchecked";

export interface SetupItem {
  id: string;
  label: string;
  need: SetupNeed;
  state: SetupState;
  detail: string;
}

/** Mirrors Rust `hermes_setup::SetupReport`. */
export interface CoreSetupReport {
  items: SetupItem[];
  missingRequired: number;
  firstRunDefaults: string[];
  /** owner 2026-11-14: node tools on for new installs only. null = not decided yet. */
  install?: "new" | "upgrade" | null;
}

export interface HermesSetupState {
  /** null = not checked yet. */
  items: SetupItem[] | null;
  missingRequired: number;
  firstRunDefaults: string[];
  install: "new" | "upgrade" | null;
  /** Every required item verified, Hermes's loaded skills included (so only while it runs). */
  complete: boolean;
  /** The core report could not be read (the rows above are then only the app's own). */
  error: string | null;
  checkedAt: number | null;
}

export const hermesSetupSlice = createSlice<HermesSetupState>({ items: null, missingRequired: 0, firstRunDefaults: [], install: null, complete: false, error: null, checkedAt: null });

export interface SelfCheckDeps {
  core: () => Promise<CoreSetupReport>;
  status: () => Promise<{ running: boolean; skills: number }>;
  tools: () => readonly { function: { name: string }; annotations?: unknown }[];
  prompt: () => string;
  now: () => number;
}

const defaultDeps: SelfCheckDeps = {
  core: () =>
    BRIDGE_MODE === "tauri" ? invoke<CoreSetupReport>("hermes_setup_check") : Promise.reject(new Error("the setup check runs in the desktop app")),
  status: () => bridge.agentHarness.status(),
  tools: () => annotatedAgentTools(),
  prompt: () => AGENT_SYSTEM_PROMPT,
  now: () => Date.now(),
};

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The rows the app checks itself. */
export async function appItems(deps: Pick<SelfCheckDeps, "status" | "tools" | "prompt">): Promise<SetupItem[]> {
  const out: SetupItem[] = [];
  const tools = deps.tools();
  const bare = tools.filter((t) => t.annotations == null).map((t) => t.function.name);
  out.push(
    tools.length > 0 && bare.length === 0
      ? { id: "agent-tools", label: "Hermes tools", need: "required", state: "ok", detail: `${tools.length} tools, each with its approval rule` }
      : {
          id: "agent-tools",
          label: "Hermes tools",
          need: "required",
          state: "missing",
          detail: tools.length === 0 ? "no tools are defined for Hermes sessions" : `no approval rule for ${bare.join(", ")}`,
        },
  );
  out.push(
    deps.prompt().trim().length > 0
      ? { id: "system-prompt", label: "Hermes instructions", need: "required", state: "ok", detail: "the system prompt is in this build" }
      : { id: "system-prompt", label: "Hermes instructions", need: "required", state: "missing", detail: "the system prompt is empty in this build" },
  );
  try {
    const st = await deps.status();
    out.push(
      !st.running
        ? { id: "skills-loaded", label: "Skills loaded by Hermes", need: "required", state: "unchecked", detail: "not checked: Hermes is not running (start it from the Agent page)" }
        : st.skills > 0
          ? { id: "skills-loaded", label: "Skills loaded by Hermes", need: "required", state: "ok", detail: `Hermes reports ${st.skills} skill${st.skills === 1 ? "" : "s"}` }
          : { id: "skills-loaded", label: "Skills loaded by Hermes", need: "required", state: "missing", detail: "Hermes is running but reports no skills" },
    );
  } catch (e) {
    out.push({ id: "skills-loaded", label: "Skills loaded by Hermes", need: "required", state: "missing", detail: `Hermes's status could not be read (${msg(e)})` });
  }
  return out;
}

/** Run the self-check and put the result in the slice. Never throws. */
export async function runHermesSelfCheck(deps: SelfCheckDeps = defaultDeps): Promise<HermesSetupState> {
  let core: CoreSetupReport | null = null;
  let error: string | null = null;
  try {
    core = await deps.core();
  } catch (e) {
    error = msg(e);
  }
  const own = await appItems(deps);
  const items = [...(core?.items ?? []), ...own];
  const missingRequired = items.filter((i) => i.need === "required" && i.state === "missing").length + (error ? 1 : 0);
  const complete = !error && missingRequired === 0 && items.every((i) => i.need !== "required" || i.state !== "unchecked");
  const next: HermesSetupState = {
    items,
    missingRequired,
    firstRunDefaults: core?.firstRunDefaults ?? [],
    install: core?.install ?? null,
    complete,
    error,
    checkedAt: deps.now(),
  };
  hermesSetupSlice.set(next);
  return next;
}

/** The card's header word: complete only when every required item was verified. */
export function setupSummary(s: Pick<HermesSetupState, "items" | "missingRequired" | "complete" | "error">): string {
  if (s.items === null) return "checking";
  if (s.error) return "not checked";
  if (s.missingRequired > 0) return `${s.missingRequired} missing`;
  return s.complete ? "complete" : "not fully checked: Hermes is not running";
}

/** Whether to offer "Turn on node tools" (the node row is off, e.g. an upgraded install). */
export function offerNodeTools(s: Pick<HermesSetupState, "items">): boolean {
  return (s.items ?? []).some((i) => i.id === "node-mcp" && i.state === "off");
}

/** Whether to offer the explicit repair (folders or capsules missing). */
export function offerRepair(s: Pick<HermesSetupState, "items">): boolean {
  return (s.items ?? []).some((i) => (i.id === "data-folders" || i.id === "starter-capsules") && i.state === "missing");
}

/** Run a core setup action (repair, turn on node tools), then the whole check. Returns its error. */
export async function runSetupAction(cmd: "hermes_setup_repair" | "hermes_setup_enable_node_tools", deps: SelfCheckDeps = defaultDeps): Promise<string | null> {
  try {
    if (BRIDGE_MODE !== "tauri") throw new Error("setup actions run in the desktop app");
    await invoke<CoreSetupReport>(cmd);
  } catch (e) {
    return msg(e);
  }
  await runHermesSelfCheck(deps);
  return null;
}

/** The launch notice when something Hermes needs is missing (null = nothing to say). */
export function setupNotice(s: Pick<HermesSetupState, "items" | "missingRequired" | "error">): string | null {
  if (s.error) return `Hermes setup could not be checked: ${s.error}. See Agent › Setup.`;
  if (s.missingRequired === 0) return null;
  const names = (s.items ?? []).filter((i) => i.need === "required" && i.state === "missing").map((i) => i.label);
  return `Hermes is missing ${names.length === 1 ? "one thing it needs" : `${names.length} things it needs`}: ${names.join(", ")}. See Agent › Setup.`;
}
