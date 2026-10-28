// Owner decision 2026-10-19 (supersedes 2026-10-04): Hermes terminal commands (the sidecar's
// shell_run) are OFF by default for new installs. Existing installs keep whatever they stored; only
// an install with no stored value takes the new default. Every command still asks the member first,
// runs in the OS sandbox and only in shared folders. The switch reaches core through
// `hermes_terminal_set`.
import { beforeEach, describe, expect, it, vi } from "vitest";
import { loadState, STORAGE_KEY } from "./state";
import {
  TERMINAL_ACCESS_HINT,
  TERMINAL_ACCESS_LABEL,
  syncTerminalAccess,
  type TerminalAccessIo,
} from "../agent/terminalAccess";
import { AGENT_SYSTEM_PROMPT } from "../agent/harness";

describe("Hermes terminal commands default", () => {
  beforeEach(() => localStorage.clear());

  it("is off for a fresh install", () => {
    expect(loadState().hermesTerminal).toBe(false);
  });

  it("is off for an install saved before the setting existed (no stored value)", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ hermesSidecarLoop: true, hermesSidecarLoopDefaultApplied: true }));
    expect(loadState().hermesTerminal).toBe(false);
  });

  it("keeps a stored off, with or without the old marker", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ hermesTerminal: false }));
    expect(loadState().hermesTerminal).toBe(false);
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ hermesTerminal: false, hermesTerminalDefaultApplied: true }));
    expect(loadState().hermesTerminal).toBe(false);
  });

  it("keeps an existing install's stored on (the 2026-10-04 default or the member's choice)", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ hermesTerminal: true, hermesTerminalDefaultApplied: true }));
    expect(loadState().hermesTerminal).toBe(true);
  });

  it("keeps a stored on that predates the marker", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ hermesTerminal: true }));
    expect(loadState().hermesTerminal).toBe(true);
  });
});

describe("Hermes terminal commands reach core", () => {
  it("sends the choice to hermes_terminal_set in the desktop app", async () => {
    const invoke = vi.fn().mockResolvedValue({ enabled: false, restarted: true });
    const io: TerminalAccessIo = { mode: "tauri", invoke: invoke as TerminalAccessIo["invoke"] };
    const st = await syncTerminalAccess(io, false);
    expect(invoke).toHaveBeenCalledWith("hermes_terminal_set", { enabled: false });
    expect(st).toEqual({ enabled: false, restarted: true });
  });

  it("sends nothing in the web preview (no sidecar there)", async () => {
    const invoke = vi.fn();
    const io: TerminalAccessIo = { mode: "sim", invoke: invoke as TerminalAccessIo["invoke"] };
    expect(await syncTerminalAccess(io, true)).toEqual({ enabled: true, restarted: false });
    expect(invoke).not.toHaveBeenCalled();
  });

  it("the copy says every command asks first, sandboxed, shared folders only, without em-dashes", () => {
    expect(TERMINAL_ACCESS_LABEL).toBe("Let Hermes run terminal commands");
    expect(TERMINAL_ACCESS_HINT).toMatch(/every command asks you first/i);
    expect(TERMINAL_ACCESS_HINT).toMatch(/sandbox/i);
    expect(TERMINAL_ACCESS_HINT).toMatch(/folders you've shared/i);
    expect(TERMINAL_ACCESS_HINT).not.toMatch(/—/);
    expect(TERMINAL_ACCESS_HINT).toMatch(/^Off by default\./);
  });

  it("Hermes's prompt tells the model shell_run exists and needs a shared folder", () => {
    expect(AGENT_SYSTEM_PROMPT).toContain("shell_run");
    expect(AGENT_SYSTEM_PROMPT).toMatch(/shared a folder/);
    expect(AGENT_SYSTEM_PROMPT).toMatch(/approves every command/);
  });
});
