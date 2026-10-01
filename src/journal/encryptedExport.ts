// =====================================================================
// HUP-S10.4 — encrypted journal export / import (webview half).
//
// The journal bundle is built here and handed, in memory, to the Rust command
// `journal_export_encrypted`, which seals it (the custody vault's Argon2id
// derivation + AES-256-GCM, versioned header) and writes ONLY ciphertext to the
// path the member picked. Import picks a file, Rust opens it with the
// passphrase, and the bundle is validated and merged here without overwriting
// any local page. Nothing leaves the machine unless the member exports it.
//
// The web preview has no sealer, so it says so instead of pretending (Rule 1).
// =====================================================================
import type { JournalPage } from "../shell/state";
import { BundleError, buildBundle, mergeImported, parseBundle } from "./bundle";

export const JOURNAL_EXT = "citrate-journal";
export const MIN_PASSPHRASE_CHARS = 12;
const DESKTOP_ONLY = "Encrypted journal files need the desktop app; the web preview cannot seal or open them.";

/** The IO seam: the runtime, the native file dialogs and the Rust commands. */
export interface JournalIo {
  mode: "tauri" | "sim";
  pickSavePath(defaultName: string): Promise<string | null>;
  pickOpenPath(): Promise<string | null>;
  invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T>;
}

export type ExportResult = { ok: true; path: string; bytes: number } | { ok: false; reason: string; cancelled?: boolean };
export type ImportResult =
  | { ok: true; pages: JournalPage[]; added: number; unchanged: number; copied: number }
  | { ok: false; reason: string; cancelled?: boolean };

function message(e: unknown): string {
  if (e instanceof Error) return e.message;
  return String(e);
}

/** The passphrase rule for an export, as a member-facing message (null = fine). */
export function checkExportPassphrase(passphrase: string, confirm: string): string | null {
  if ([...passphrase].length < MIN_PASSPHRASE_CHARS) return `Use a passphrase of at least ${MIN_PASSPHRASE_CHARS} characters.`;
  if (passphrase !== confirm) return "The two passphrases do not match.";
  return null;
}

function withExtension(path: string): string {
  // Exact, case-sensitive match: the Rust command accepts only this spelling.
  return path.endsWith("." + JOURNAL_EXT) ? path : path + "." + JOURNAL_EXT;
}

/** Seal the whole journal into a file the member picks. */
export async function exportJournalEncrypted(io: JournalIo, pages: JournalPage[], passphrase: string, confirm: string, now: Date): Promise<ExportResult> {
  if (io.mode !== "tauri") return { ok: false, reason: DESKTOP_ONLY };
  const bad = checkExportPassphrase(passphrase, confirm);
  if (bad) return { ok: false, reason: bad };
  const day = now.toISOString().slice(0, 10);
  let picked: string | null;
  try {
    picked = await io.pickSavePath(`citrate-journal-${day}.${JOURNAL_EXT}`);
  } catch (e) {
    return { ok: false, reason: "Could not open the save dialog: " + message(e) };
  }
  if (!picked) return { ok: false, cancelled: true, reason: "Export cancelled." };
  const path = withExtension(picked);
  try {
    const bytes = await io.invoke<number>("journal_export_encrypted", { path, passphrase, bundle: buildBundle(pages, now.toISOString()) });
    return { ok: true, path, bytes };
  } catch (e) {
    return { ok: false, reason: message(e) };
  }
}

/** Open an exported file and merge it into `existing` (never overwriting). */
export async function importJournalEncrypted(io: JournalIo, existing: JournalPage[], passphrase: string): Promise<ImportResult> {
  if (io.mode !== "tauri") return { ok: false, reason: DESKTOP_ONLY };
  if (!passphrase) return { ok: false, reason: "Enter the passphrase the file was exported with." };
  let path: string | null;
  try {
    path = await io.pickOpenPath();
  } catch (e) {
    return { ok: false, reason: "Could not open the file dialog: " + message(e) };
  }
  if (!path) return { ok: false, cancelled: true, reason: "Import cancelled." };
  let text: string;
  try {
    text = await io.invoke<string>("journal_import_encrypted", { path, passphrase });
  } catch (e) {
    return { ok: false, reason: message(e) };
  }
  try {
    const merged = mergeImported(existing, parseBundle(text));
    return { ok: true, ...merged };
  } catch (e) {
    return { ok: false, reason: e instanceof BundleError ? e.message : "The file could not be read as a journal." };
  }
}

/** The production IO: the bridge's runtime mode, the Tauri dialog plugin and the timeout-wrapped invoke. */
export async function desktopJournalIo(): Promise<JournalIo> {
  const { BRIDGE_MODE } = await import("../bridge/mode");
  const filters = [{ name: "Citrate journal", extensions: [JOURNAL_EXT] }];
  return {
    mode: BRIDGE_MODE,
    pickSavePath: async (defaultPath) => {
      const { save } = await import("@tauri-apps/plugin-dialog");
      return save({ defaultPath, filters });
    },
    pickOpenPath: async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ multiple: false, directory: false, filters });
      return typeof picked === "string" ? picked : null;
    },
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      const { invoke } = await import("../bridge/tauri/invoke");
      return invoke<T>(cmd, args);
    },
  };
}
