// =====================================================================
// HGW-W11 + the member's commit identity: how Hermes builds in a project (Settings side).
//
// Core (src-tauri/src/hermes_build_settings.rs) stores the choices and pins, for the Hermes
// sidecar at each start, CITRATE_HERMES_AUTO_APPROVE (`0` = off, empty = the shipped list) and
// CITRATE_HERMES_GIT_USER_{NAME,EMAIL}. The list itself is the sidecar's own (runtime
// agent-sidecar/policy/auto_approve.toml), read live from the running sidecar
// (`GET /shell/auto-approve`); this panel shows it and never edits it.
// =====================================================================

/** What `hermes_build_settings_get` / `_set` return (mirrors the Rust struct). */
export interface BuildSettingsStatus {
  autoApprove: boolean;
  gitName: string | null;
  gitEmail: string | null;
  /** True when a running Hermes was restarted to apply the change. */
  restarted: boolean;
  /** A note when the saved settings could not be read (auto-approve then counts as off). */
  loadError: string | null;
}

/** One listed command template, as the sidecar reports it. */
export interface AutoApproveRule {
  id: string;
  program: string;
  subcommands?: string[];
  flags?: string[];
  require_one_of?: string[];
  operands?: string;
  operand_values?: string[];
}

/** What `hermes_auto_approve_list` returns: the list in effect in the running Hermes. */
export interface AutoApproveListView {
  enabled: boolean;
  conditions: string[];
  rules: AutoApproveRule[];
}

export interface BuildSettingsIo {
  mode: "tauri" | "sim";
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
}

export const AUTO_APPROVE_LABEL = "Run listed project commands without asking";

/** No em-dashes (house style). */
export const AUTO_APPROVE_HINT =
  "On by default. A command on this list runs without a card only when the conversation has not read outside content, " +
  "the sandbox is on (no network), and the folder is one of Hermes's projects. Everything else, and every command after " +
  "outside content, still asks you. Each run is recorded. Changing this restarts Hermes.";

export const IDENTITY_HINT =
  "Hermes's commits use this name and email. Leave both empty to use your git config (or \"Hermes\" when there is none). " +
  "Every Hermes commit also says Co-authored-by: Hermes.";

/** How one rule reads for a person, e.g. `npm run build|test|lint` or `forge build|test`. */
export function ruleText(r: AutoApproveRule): string {
  const parts = [r.program];
  const subs = r.subcommands ?? [];
  if (subs.length > 0) parts.push(subs.join(" "));
  const values = r.operand_values ?? [];
  if (values.length > 0) parts.push(values.join("|"));
  else if (r.operands && r.operands !== "none") parts.push(`<${r.operands}>`);
  const req = r.require_one_of ?? [];
  if (req.length > 0) parts.push(`(with ${req.join(" or ")})`);
  return parts.join(" ");
}

/** The stored choices; the web preview has none (no sidecar). */
export async function readBuildSettings(io: BuildSettingsIo): Promise<BuildSettingsStatus | null> {
  if (io.mode !== "tauri") return null;
  return io.invoke<BuildSettingsStatus>("hermes_build_settings_get");
}

export async function writeBuildSettings(
  io: BuildSettingsIo,
  autoApprove: boolean,
  gitName: string,
  gitEmail: string,
): Promise<BuildSettingsStatus> {
  return io.invoke<BuildSettingsStatus>("hermes_build_settings_set", {
    autoApprove,
    gitName: gitName.trim() === "" ? null : gitName,
    gitEmail: gitEmail.trim() === "" ? null : gitEmail,
  });
}

export async function readAutoApproveList(io: BuildSettingsIo): Promise<AutoApproveListView> {
  return io.invoke<AutoApproveListView>("hermes_auto_approve_list");
}

export async function desktopBuildSettingsIo(): Promise<BuildSettingsIo> {
  const { BRIDGE_MODE } = await import("../../bridge/mode");
  return {
    mode: BRIDGE_MODE,
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>) => {
      const { invoke } = await import("../../bridge/tauri/invoke");
      return invoke<T>(cmd, args);
    },
  };
}
