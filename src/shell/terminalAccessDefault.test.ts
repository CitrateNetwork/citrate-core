// Owner decision 2026-10-04: Hermes may run terminal commands (the sidecar's shell_run) by default.
// Every command still asks the member first, runs in the OS sandbox and only in shared folders.
// Installs that saved state before the setting existed are switched on once; a member's later
// choice is kept. The switch reaches core through `hermes_terminal_set`.
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

  it("is on for a fresh install", () => {
    const s = loadState();
    expect(s.hermesTerminal).toBe(true);
    expect(s.hermesTerminalDefaultApplied).toBe(true);
  });

  it("switches an install saved before the setting existed on, once", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ hermesSidecarLoop: true, hermesSidecarLoopDefaultApplied: true }));
    const s = loadState();
    expect(s.hermesTerminal).toBe(true);
    expect(s.hermesTerminalDefaultApplied).toBe(true);
  });

  it("switches an install that saved it off without the marker on, once", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ hermesTerminal: false }));
    expect(loadState().hermesTerminal).toBe(true);
  });

  it("keeps a member's later choice to turn it off", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ hermesTerminal: false, hermesTerminalDefaultApplied: true }));
    expect(loadState().hermesTerminal).toBe(false);
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
  });

  it("Hermes's prompt tells the model shell_run exists and needs a shared folder", () => {
    expect(AGENT_SYSTEM_PROMPT).toContain("shell_run");
    expect(AGENT_SYSTEM_PROMPT).toMatch(/shared a folder/);
    expect(AGENT_SYSTEM_PROMPT).toMatch(/approves every command/);
  });
});
