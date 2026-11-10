// v0.5.0-rc.5 FIX 5 — the contract-toolchain refusal becomes an actionable message.
//
// The Hermes sidecar (runtime 626486b8, and 05243c72 the same) refuses a workflow with "this workflow needs
// the contract toolchain (aderyn_scan, forge_test, medusa_fuzz, slither_scan), which is off in
// this app". The app now reads core's own toolchain status and says what to do: turn the switch
// on, install each missing tool (with its command for this OS), or restart Hermes.
import { describe, it, expect } from "vitest";
import {
  actionableWorkflowError,
  hostOs,
  INSTALL,
  isToolchainRefusal,
  shortToolchainHint,
  toolchainAdvice,
  TOOLCHAIN_SWITCH_PATH,
  WINDOWS_UNSUPPORTED,
} from "./toolchainGuidance";
import type { ToolchainStatus } from "./contractForge";

const REFUSAL =
  "this workflow needs the contract toolchain (aderyn_scan, forge_test, medusa_fuzz, slither_scan), which is off in this app";

function status(enabled: boolean, found: Partial<Record<"forge" | "slither" | "aderyn" | "medusa", boolean>>, solc = true): ToolchainStatus {
  const prog = (program: string, tool: string) => ({ program, tool, path: found[program as keyof typeof found] ? `/x/${program}` : null });
  return {
    settings: { enabled },
    programs: [prog("forge", "forge_test"), prog("slither", "slither_scan"), prog("aderyn", "aderyn_scan"), prog("medusa", "medusa_fuzz")],
    solc: solc ? "/x/solc-0.8.36" : null,
    searchPath: [],
    notices: [],
    appliesOnRestart: true,
    loadError: null,
  };
}
const ALL = { forge: true, slither: true, aderyn: true, medusa: true };

describe("recognising the refusal", () => {
  it("matches the sidecar's toolchain refusal, with or without the 'this workflow' prefix", () => {
    expect(isToolchainRefusal(REFUSAL)).toBe(true);
    expect(isToolchainRefusal("needs the contract toolchain (forge_test), which is off in this app")).toBe(true);
    expect(isToolchainRefusal("this workflow needs journal_read, which this conversation does not offer")).toBe(false);
  });
  it("leaves any other error untouched", () => {
    expect(actionableWorkflowError("the session refused the workflow", status(false, {}), "macos")).toBe("the session refused the workflow");
  });
});

describe("toolchainAdvice", () => {
  it("switch off: says where the switch is and to restart Hermes", () => {
    const a = toolchainAdvice(status(false, ALL), "macos");
    expect(a.kind).toBe("off");
    expect(a.text).toBe(`Turn on ${TOOLCHAIN_SWITCH_PATH}, then restart Hermes.`);
    expect(a.steps).toEqual([]);
  });

  it("switch off with tools missing: the switch AND the install steps", () => {
    const a = toolchainAdvice(status(false, { forge: true }), "macos");
    expect(a.kind).toBe("off");
    expect(a.steps.map((s) => s.program)).toEqual(["slither", "aderyn", "medusa"]);
  });

  it("switch on, tools missing: one install step per missing tool, for this OS", () => {
    const a = toolchainAdvice(status(true, { forge: true, slither: true }, false), "linux");
    expect(a.kind).toBe("missing");
    expect(a.steps.map((s) => s.program)).toEqual(["aderyn", "medusa", "solc"]);
    for (const s of a.steps) expect(s.commands.length).toBeGreaterThan(0);
    expect(a.text).toMatch(/restart Hermes/);
  });

  it("switch on, everything found: only a restart is needed", () => {
    const a = toolchainAdvice(status(true, ALL), "macos");
    expect(a.kind).toBe("restart");
    expect(a.text).toMatch(/Restart Hermes/);
  });

  it("Windows: says plainly that 0.5.0 does not support it (no commands that cannot work)", () => {
    const a = toolchainAdvice(status(true, {}), "windows");
    expect(a.kind).toBe("windows");
    expect(a.text).toBe(WINDOWS_UNSUPPORTED);
    expect(a.steps).toEqual([]);
  });

  it("no status (core unreachable): still names the switch, never claims a tool is installed", () => {
    const a = toolchainAdvice(null, "macos");
    expect(a.kind).toBe("unknown");
    expect(a.text).toContain(TOOLCHAIN_SWITCH_PATH);
  });
});

describe("install commands (checked against each project's docs, 2026-10-13)", () => {
  it("forge uses foundryup on macOS and Linux", () => {
    for (const os of ["macos", "linux"] as const) {
      expect(INSTALL.forge[os].commands).toEqual(["curl -L https://foundry.paradigm.xyz | bash", "foundryup"]);
    }
  });
  it("slither installs with pipx (the app looks for crytic-compile, which medusa needs, in pipx's slither environment)", () => {
    expect(INSTALL.slither.macos.commands).toEqual(["pipx install slither-analyzer"]);
    expect(INSTALL.slither.linux.commands).toEqual(["pipx install slither-analyzer"]);
  });
  it("aderyn: Homebrew on macOS, Cyfrin's installer on Linux", () => {
    expect(INSTALL.aderyn.macos.commands).toEqual(["brew install cyfrin/tap/aderyn"]);
    expect(INSTALL.aderyn.linux.commands[0]).toMatch(/aderyn-installer\.sh \| bash$/);
  });
  it("medusa: Homebrew on macOS, go install on Linux, and it needs slither's crytic-compile", () => {
    expect(INSTALL.medusa.macos.commands).toEqual(["brew install medusa"]);
    expect(INSTALL.medusa.linux.commands).toEqual(["go install github.com/crytic/medusa@latest"]);
    expect(INSTALL.medusa.macos.note).toMatch(/crytic-compile/);
  });
  it("solc 0.8.36 via svm", () => {
    expect(INSTALL.solc.macos.commands).toEqual(["cargo install svm-rs", "svm install 0.8.36"]);
  });
});

describe("the chat message", () => {
  it("turns the refusal into the switch instruction when the switch is off", () => {
    const out = actionableWorkflowError(REFUSAL, status(false, ALL), "macos");
    expect(out).toContain(`Turn on ${TOOLCHAIN_SWITCH_PATH}, then restart Hermes.`);
    expect(out).not.toContain("which is off in this app");
  });
  it("lists each missing tool with its command when the switch is on", () => {
    const out = actionableWorkflowError(REFUSAL, status(true, { forge: true, slither: true, aderyn: true }), "macos");
    expect(out).toContain("medusa: brew install medusa");
    expect(out).not.toContain("forge:");
  });
  it("persona picker: a short hint instead of the raw refusal", () => {
    expect(shortToolchainHint("needs the contract toolchain (forge_test), which is off in this app")).toBe(
      `needs the contract toolchain; turn it on in ${TOOLCHAIN_SWITCH_PATH} and restart Hermes`,
    );
    expect(shortToolchainHint("something else")).toBe("something else");
  });
  it("no em-dashes in any member-facing copy", () => {
    const all = [WINDOWS_UNSUPPORTED, TOOLCHAIN_SWITCH_PATH, actionableWorkflowError(REFUSAL, status(true, {}, false), "linux"), actionableWorkflowError(REFUSAL, null, "macos")];
    for (const t of all) expect(t).not.toContain("—");
  });
});

describe("hostOs", () => {
  it("reads the platform", () => {
    expect(hostOs("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")).toBe("macos");
    expect(hostOs("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe("windows");
    expect(hostOs("Mozilla/5.0 (X11; Linux x86_64)")).toBe("linux");
  });
});
