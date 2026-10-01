// HUP-S10.4 — the webview half of the encrypted export: passphrase checks, the
// save/open dialogs, and the two Rust commands, driven through an injected IO seam.
import { describe, it, expect } from "vitest";
import type { JournalPage } from "../shell/state";
import { buildBundle } from "./bundle";
import { JOURNAL_EXT, checkExportPassphrase, exportJournalEncrypted, importJournalEncrypted, type JournalIo } from "./encryptedExport";

const PASS = "a long journal passphrase";
const pages: JournalPage[] = [{ id: "d-2026-10-01", title: "2026-10-01", kind: "daily", pinned: false, blocks: ["hello"] }];
const NOW = new Date("2026-10-01T04:05:06Z");

interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

function io(over: Partial<JournalIo> & { reply?: unknown; fail?: string } = {}): JournalIo & { calls: Call[] } {
  const calls: Call[] = [];
  return {
    mode: "tauri",
    pickSavePath: async () => "/Users/me/journal",
    pickOpenPath: async () => "/Users/me/journal.citrate-journal",
    invoke: async <T,>(cmd: string, args: Record<string, unknown>): Promise<T> => {
      calls.push({ cmd, args });
      if (over.fail) throw new Error(over.fail);
      return over.reply as T;
    },
    ...over,
    calls,
  };
}

describe("checkExportPassphrase", () => {
  it("needs at least 12 characters and a matching confirmation", () => {
    expect(checkExportPassphrase("short", "short")).toMatch(/12 characters/);
    expect(checkExportPassphrase(PASS, PASS + "x")).toMatch(/do not match/);
    expect(checkExportPassphrase(PASS, PASS)).toBeNull();
  });
});

describe("exportJournalEncrypted", () => {
  it("hands the bundle and passphrase to the Rust sealer with a .citrate-journal path", async () => {
    const x = io({ reply: 321 });
    const r = await exportJournalEncrypted(x, pages, PASS, PASS, NOW);
    expect(r).toEqual({ ok: true, path: "/Users/me/journal." + JOURNAL_EXT, bytes: 321 });
    expect(x.calls).toHaveLength(1);
    expect(x.calls[0].cmd).toBe("journal_export_encrypted");
    expect(x.calls[0].args).toEqual({ path: "/Users/me/journal." + JOURNAL_EXT, passphrase: PASS, bundle: buildBundle(pages, NOW.toISOString()) });
  });

  it("keeps an extension the member already typed", async () => {
    const x = io({ reply: 1, pickSavePath: async () => "/a/b.citrate-journal" });
    const r = await exportJournalEncrypted(x, pages, PASS, PASS, NOW);
    expect(r.ok && r.path).toBe("/a/b.citrate-journal");
  });

  it("does nothing when the passphrase is weak (no dialog, no invoke)", async () => {
    let asked = false;
    const x = io({ pickSavePath: async () => ((asked = true), "/a") });
    const r = await exportJournalEncrypted(x, pages, "short", "short", NOW);
    expect(r.ok).toBe(false);
    expect(asked).toBe(false);
    expect(x.calls).toHaveLength(0);
  });

  it("treats a cancelled save dialog as a quiet cancel", async () => {
    const x = io({ pickSavePath: async () => null });
    const r = await exportJournalEncrypted(x, pages, PASS, PASS, NOW);
    expect(r).toEqual({ ok: false, cancelled: true, reason: "Export cancelled." });
    expect(x.calls).toHaveLength(0);
  });

  it("says plainly that the web preview cannot export encrypted files", async () => {
    const x = io({ mode: "sim" });
    const r = await exportJournalEncrypted(x, pages, PASS, PASS, NOW);
    expect(r.ok).toBe(false);
    expect(!r.ok && r.reason).toMatch(/desktop app/);
    expect(x.calls).toHaveLength(0);
  });

  it("passes the Rust error message through", async () => {
    const x = io({ fail: "The file could not be read or written." });
    const r = await exportJournalEncrypted(x, pages, PASS, PASS, NOW);
    expect(r).toEqual({ ok: false, reason: "The file could not be read or written." });
  });
});

describe("importJournalEncrypted", () => {
  it("opens, validates and merges without overwriting local pages", async () => {
    const incoming: JournalPage[] = [
      { ...pages[0], blocks: ["from the file"] },
      { id: "p-new", title: "New", kind: "page", pinned: true, blocks: ["n"] },
    ];
    const x = io({ reply: buildBundle(incoming, NOW.toISOString()) });
    const r = await importJournalEncrypted(x, pages, PASS);
    expect(x.calls[0]).toEqual({ cmd: "journal_import_encrypted", args: { path: "/Users/me/journal.citrate-journal", passphrase: PASS } });
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.pages[0]).toEqual(pages[0]);
    expect(r.added).toBe(1);
    expect(r.copied).toBe(1);
  });

  it("reports a wrong passphrase from Rust cleanly", async () => {
    const x = io({ fail: "That passphrase does not open this file, or the file was changed or damaged." });
    const r = await importJournalEncrypted(x, pages, "nope");
    expect(r).toEqual({ ok: false, reason: "That passphrase does not open this file, or the file was changed or damaged." });
  });

  it("rejects a decrypted payload that is not a journal bundle", async () => {
    const x = io({ reply: JSON.stringify({ format: "other" }) });
    const r = await importJournalEncrypted(x, pages, PASS);
    expect(r.ok).toBe(false);
    expect(!r.ok && r.reason).toMatch(/not a Citrate journal/);
  });

  it("needs a passphrase and the desktop app", async () => {
    expect((await importJournalEncrypted(io(), pages, "")).ok).toBe(false);
    const sim = await importJournalEncrypted(io({ mode: "sim" }), pages, PASS);
    expect(!sim.ok && sim.reason).toMatch(/desktop app/);
  });

  it("treats a cancelled open dialog as a quiet cancel", async () => {
    const r = await importJournalEncrypted(io({ pickOpenPath: async () => null }), pages, PASS);
    expect(r).toEqual({ ok: false, cancelled: true, reason: "Import cancelled." });
  });
});
