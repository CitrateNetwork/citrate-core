// =====================================================================
// v0.5.0-rc.5 FIX 5 — what to do when Hermes's contract toolchain is unavailable.
//
// The Hermes sidecar refuses a workflow that needs forge/slither/aderyn/medusa with "this workflow
// needs the contract toolchain (...), which is off in this app" (citrate-agent-runtime
// agent-sidecar, unchanged). Core already knows why (`toolchain_settings_get`: the switch, which
// programs it found, the solc 0.8.36 path), so the app turns that into an actionable message:
// turn the switch on, install each missing tool with its command for this OS, or restart Hermes.
//
// The app does not install these tools in 0.5.0. The commands come from each project's own docs
// (checked 2026-10-13): Foundry README (foundryup), Slither README (pip/uv/brew; pipx is used here
// because core looks for crytic-compile, which medusa needs, inside pipx's slither environment),
// Aderyn README (Homebrew tap, Cyfrin installer script), Medusa installation guide (Homebrew,
// go install, release binaries), svm-rs README (cargo install svm-rs, svm install <version>).
//
// Windows: core's tool search looks for the Unix program names in Unix folders, so it cannot find
// these tools on Windows; Aderyn's own docs require WSL. 0.5.0 says so rather than listing
// commands that would not make the toolchain work.
// =====================================================================
import type { ToolchainStatus } from "./contractForge";

export type HostOs = "macos" | "linux" | "windows";

/** Where the switch lives in the app. */
export const TOOLCHAIN_SWITCH_PATH = "Agent > Contracts > Toolchain";

export const WINDOWS_UNSUPPORTED =
  "The contract toolchain is not supported on Windows in 0.5.0: the app cannot find or run forge, slither, aderyn or medusa there yet.";

export interface InstallHint {
  commands: string[];
  note?: string;
}

export type ToolKey = "forge" | "slither" | "aderyn" | "medusa" | "solc";

const SLITHER_ENV_NOTE = "medusa compiles through crytic-compile, which comes with slither (install slither with pipx first).";

export const INSTALL: Record<ToolKey, { macos: InstallHint; linux: InstallHint }> = {
  forge: {
    macos: { commands: ["curl -L https://foundry.paradigm.xyz | bash", "foundryup"] },
    linux: { commands: ["curl -L https://foundry.paradigm.xyz | bash", "foundryup"] },
  },
  slither: {
    macos: { commands: ["pipx install slither-analyzer"], note: "Needs Python 3.10 or newer." },
    linux: { commands: ["pipx install slither-analyzer"], note: "Needs Python 3.10 or newer." },
  },
  aderyn: {
    macos: { commands: ["brew install cyfrin/tap/aderyn"] },
    linux: {
      commands: ["curl --proto '=https' --tlsv1.2 -LsSf https://github.com/cyfrin/aderyn/releases/latest/download/aderyn-installer.sh | bash"],
    },
  },
  medusa: {
    macos: { commands: ["brew install medusa"], note: SLITHER_ENV_NOTE },
    linux: {
      commands: ["go install github.com/crytic/medusa@latest"],
      note: "Or download the release binary from github.com/crytic/medusa/releases. " + SLITHER_ENV_NOTE,
    },
  },
  solc: {
    macos: { commands: ["cargo install svm-rs", "svm install 0.8.36"], note: "Or run forge build once in a project pinned to solc 0.8.36." },
    linux: { commands: ["cargo install svm-rs", "svm install 0.8.36"], note: "Or run forge build once in a project pinned to solc 0.8.36." },
  },
};

export interface InstallStep {
  program: ToolKey;
  /** The Hermes tool it serves (forge_test, ...); "compiler" for solc. */
  tool: string;
  commands: string[];
  note?: string;
}

export type AdviceKind = "off" | "missing" | "restart" | "windows" | "unknown";

export interface ToolchainAdvice {
  kind: AdviceKind;
  text: string;
  steps: InstallStep[];
}

const REFUSAL_RE = /needs the contract toolchain \(([^)]*)\), which is off in this app/;

export function isToolchainRefusal(text: string): boolean {
  return REFUSAL_RE.test(text);
}

/** The member's OS, from the user agent (the desktop webview reports the host platform). */
export function hostOs(ua: string = typeof navigator !== "undefined" ? navigator.userAgent : ""): HostOs {
  if (/Windows/i.test(ua)) return "windows";
  if (/Mac OS X|Macintosh/i.test(ua)) return "macos";
  return "linux";
}

/** Install steps for every program core did not find (plus solc 0.8.36 when it is missing). */
export function missingSteps(st: ToolchainStatus, os: Exclude<HostOs, "windows">): InstallStep[] {
  const out: InstallStep[] = [];
  for (const p of st.programs) {
    if (p.path) continue;
    const key = p.program as ToolKey;
    const hint = INSTALL[key]?.[os];
    if (hint) out.push({ program: key, tool: p.tool, ...hint });
  }
  if (!st.solc) out.push({ program: "solc", tool: "compiler", ...INSTALL.solc[os] });
  return out;
}

export function toolchainAdvice(st: ToolchainStatus | null, os: HostOs): ToolchainAdvice {
  if (os === "windows") return { kind: "windows", text: WINDOWS_UNSUPPORTED, steps: [] };
  if (!st) {
    return {
      kind: "unknown",
      text: `Turn on ${TOOLCHAIN_SWITCH_PATH}, install the tools it lists as missing, then restart Hermes.`,
      steps: [],
    };
  }
  const steps = missingSteps(st, os);
  if (!st.settings.enabled) return { kind: "off", text: `Turn on ${TOOLCHAIN_SWITCH_PATH}, then restart Hermes.`, steps };
  if (steps.length) return { kind: "missing", text: "Install these, then restart Hermes:", steps };
  return { kind: "restart", text: "The toolchain is on and every tool was found. Restart Hermes so it picks up the change.", steps: [] };
}

/** One line per step: `program: cmd && cmd (note)`. */
export function formatSteps(steps: InstallStep[]): string[] {
  return steps.map((s) => `${s.program}: ${s.commands.join(" && ")}${s.note ? ` (${s.note})` : ""}`);
}

/** The chat error for a refused workflow: actionable for the toolchain refusal, unchanged otherwise. */
export function actionableWorkflowError(text: string, st: ToolchainStatus | null, os: HostOs): string {
  if (!isToolchainRefusal(text)) return text;
  const a = toolchainAdvice(st, os);
  const lines = ["This workflow needs the contract toolchain. " + a.text];
  if (a.steps.length) {
    if (a.kind === "off") lines.push("Not installed yet:");
    lines.push(...formatSteps(a.steps));
  }
  return lines.join("\n");
}

/** The persona picker's one-line form (it has no status to hand). */
export function shortToolchainHint(text: string): string {
  if (!isToolchainRefusal(text)) return text;
  return `needs the contract toolchain; turn it on in ${TOOLCHAIN_SWITCH_PATH} and restart Hermes`;
}
