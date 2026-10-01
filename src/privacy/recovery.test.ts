// HUP-S10.5 — recovery kit (webview half): paths, passphrase rule, and honest failures.
// Written red-first (module absent), then implemented.
import { describe, it, expect, vi } from "vitest";
import type { PrivacyIo } from "./privacyIo";
import {
  FILE_EXT,
  SHEET_SUFFIX,
  checkKitPassphrase,
  recoveryStatus,
  restoreFromFile,
  restoreFromPhrase,
  saveRecoveryFile,
  saveRecoveryPhrase,
  sheetPath,
  filePath,
} from "./recovery";

const NOW = new Date("2026-10-01T10:00:00Z");
const PASS = "a long recovery passphrase";

function io(over: Partial<PrivacyIo> = {}): PrivacyIo {
  return {
    mode: "tauri",
    pickSavePath: async () => "/Users/me/keys",
    pickOpenPath: async () => "/Users/me/keys.citrate-recovery",
    invoke: vi.fn(async () => 2) as PrivacyIo["invoke"],
    clearWebStorage: () => {},
    ...over,
  };
}

describe("recovery kit paths", () => {
  it("always ends a sheet in .citrate-recovery.txt and a file in .citrate-recovery", () => {
    expect(sheetPath("/a/keys")).toBe("/a/keys" + SHEET_SUFFIX);
    expect(sheetPath("/a/keys.txt")).toBe("/a/keys" + SHEET_SUFFIX);
    expect(sheetPath("/a/keys" + SHEET_SUFFIX)).toBe("/a/keys" + SHEET_SUFFIX);
    expect(filePath("/a/keys")).toBe("/a/keys." + FILE_EXT);
    expect(filePath("/a/keys." + FILE_EXT)).toBe("/a/keys." + FILE_EXT);
  });
});

describe("checkKitPassphrase", () => {
  it("needs 12+ characters and a matching confirmation", () => {
    expect(checkKitPassphrase("short", "short")).toMatch(/12/);
    expect(checkKitPassphrase(PASS, PASS + "x")).toMatch(/do not match/);
    expect(checkKitPassphrase(PASS, PASS)).toBeNull();
  });
});

describe("saving a kit", () => {
  it("writes the phrase sheet through Rust and never receives the words", async () => {
    const invoke = vi.fn(async () => 2);
    const r = await saveRecoveryPhrase(io({ invoke: invoke as PrivacyIo["invoke"] }), NOW);
    expect(r).toEqual({ ok: true, path: "/Users/me/keys" + SHEET_SUFFIX, keys: 2 });
    expect(invoke).toHaveBeenCalledWith("recovery_kit_save_phrase", { path: "/Users/me/keys" + SHEET_SUFFIX });
  });

  it("refuses a weak passphrase before any dialog opens", async () => {
    const pickSavePath = vi.fn(async () => "/x");
    const r = await saveRecoveryFile(io({ pickSavePath }), "short", "short", NOW);
    expect(r.ok).toBe(false);
    expect(pickSavePath).not.toHaveBeenCalled();
  });

  it("seals the recovery file with the passphrase at the picked path", async () => {
    const invoke = vi.fn(async () => 2);
    const r = await saveRecoveryFile(io({ invoke: invoke as PrivacyIo["invoke"] }), PASS, PASS, NOW);
    expect(r.ok).toBe(true);
    expect(invoke).toHaveBeenCalledWith("recovery_kit_save_file", { path: "/Users/me/keys." + FILE_EXT, passphrase: PASS });
  });

  it("a cancelled dialog is a quiet cancel, not an error", async () => {
    const r = await saveRecoveryPhrase(io({ pickSavePath: async () => null }), NOW);
    expect(r).toMatchObject({ ok: false, cancelled: true });
  });

  it("the web preview says it needs the desktop app", async () => {
    const r = await saveRecoveryPhrase(io({ mode: "sim" }), NOW);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toMatch(/desktop app/);
  });
});

describe("restoring", () => {
  it("passes the typed sheet and the replace choice to Rust", async () => {
    const invoke = vi.fn(async () => ({ restored: ["node-storage-key"], unchanged: [] }));
    const r = await restoreFromPhrase(io({ invoke: invoke as PrivacyIo["invoke"] }), "[node-storage-key] ...", false);
    expect(r).toEqual({ ok: true, restored: ["node-storage-key"], unchanged: [] });
    expect(invoke).toHaveBeenCalledWith("recovery_kit_restore_phrase", { sheet: "[node-storage-key] ...", replace: false });
  });

  it("an empty phrase never reaches Rust", async () => {
    const invoke = vi.fn();
    const r = await restoreFromPhrase(io({ invoke: invoke as PrivacyIo["invoke"] }), "   ", false);
    expect(r.ok).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("a wrong recovery comes back as Rust's plain message", async () => {
    const msg = "This recovery kit belongs to a different install. Its keys do not match the data on this computer, so nothing was changed.";
    const r = await restoreFromFile(io({ invoke: (async () => { throw new Error(msg); }) as PrivacyIo["invoke"] }), PASS, false);
    expect(r).toEqual({ ok: false, reason: msg });
  });

  it("status failures are reported, not hidden", async () => {
    const r = await recoveryStatus(io({ invoke: (async () => { throw new Error("keychain down"); }) as PrivacyIo["invoke"] }));
    expect(r).toEqual({ ok: false, reason: "keychain down" });
  });
});
