// =====================================================================
// HUP-S10.5 — device-key recovery kit (webview half).
//
// The member picks where the kit goes; Rust reads the keys from the keychain and
// writes the phrase sheet or the sealed file there directly. The words and key
// bytes never come back into the webview (I-2). Restore sends the typed sheet or
// the file path plus passphrase to Rust, which refuses a wrong kit and writes
// nothing unless every key checks out.
// =====================================================================
import { DESKTOP_ONLY, messageOf, type PrivacyIo } from "./privacyIo";

export const SHEET_SUFFIX = ".citrate-recovery.txt";
export const FILE_EXT = "citrate-recovery";
export const MIN_PASSPHRASE_CHARS = 12;

const SHEET_FILTER = { name: "Recovery phrase sheet", extensions: ["txt"] };
const FILE_FILTER = { name: "Citrate recovery file", extensions: [FILE_EXT] };

export interface KeyStatus {
  account: string;
  label: string;
  covers: string;
  present: boolean;
  fingerprint: string | null;
  recordedFingerprint: string | null;
}
export interface RecoveryStatus {
  keyringReachable: boolean;
  keys: KeyStatus[];
}
export interface RestoreReport {
  restored: string[];
  unchanged: string[];
}

type Fail = { ok: false; reason: string; cancelled?: boolean };
export type SaveResult = { ok: true; path: string; keys: number } | Fail;
export type RestoreResult = ({ ok: true } & RestoreReport) | Fail;
export type StatusResult = { ok: true; status: RecoveryStatus } | Fail;

/** The sheet path always carries the exact suffix Rust accepts. */
export function sheetPath(picked: string): string {
  if (picked.endsWith(SHEET_SUFFIX)) return picked;
  const base = picked.endsWith(".txt") ? picked.slice(0, -4) : picked;
  return base + SHEET_SUFFIX;
}

/** The sealed-file path always carries the exact extension Rust accepts. */
export function filePath(picked: string): string {
  return picked.endsWith("." + FILE_EXT) ? picked : picked + "." + FILE_EXT;
}

/** The recovery-file passphrase rule, as a member-facing message (null = fine). */
export function checkKitPassphrase(passphrase: string, confirm: string): string | null {
  if ([...passphrase].length < MIN_PASSPHRASE_CHARS) return `Use a passphrase of at least ${MIN_PASSPHRASE_CHARS} characters.`;
  if (passphrase !== confirm) return "The two passphrases do not match.";
  return null;
}

function day(now: Date): string {
  return now.toISOString().slice(0, 10);
}

export async function recoveryStatus(io: PrivacyIo): Promise<StatusResult> {
  if (io.mode !== "tauri") return { ok: false, reason: DESKTOP_ONLY };
  try {
    return { ok: true, status: await io.invoke<RecoveryStatus>("recovery_kit_status", {}) };
  } catch (e) {
    return { ok: false, reason: messageOf(e) };
  }
}

async function pick(io: PrivacyIo, name: string, filter: { name: string; extensions: string[] }): Promise<string | Fail> {
  try {
    const p = await io.pickSavePath(name, filter);
    return p ?? { ok: false, cancelled: true, reason: "Cancelled." };
  } catch (e) {
    return { ok: false, reason: "Could not open the save dialog: " + messageOf(e) };
  }
}

/** Write the phrase sheet (one 24-word phrase per device key) to a file the member picks. */
export async function saveRecoveryPhrase(io: PrivacyIo, now: Date): Promise<SaveResult> {
  if (io.mode !== "tauri") return { ok: false, reason: DESKTOP_ONLY };
  const picked = await pick(io, `citrate-device-keys-${day(now)}${SHEET_SUFFIX}`, SHEET_FILTER);
  if (typeof picked !== "string") return picked;
  const path = sheetPath(picked);
  try {
    const keys = await io.invoke<number>("recovery_kit_save_phrase", { path });
    return { ok: true, path, keys };
  } catch (e) {
    return { ok: false, reason: messageOf(e) };
  }
}

/** Seal the device keys into a passphrase-protected recovery file. */
export async function saveRecoveryFile(io: PrivacyIo, passphrase: string, confirm: string, now: Date): Promise<SaveResult> {
  if (io.mode !== "tauri") return { ok: false, reason: DESKTOP_ONLY };
  const bad = checkKitPassphrase(passphrase, confirm);
  if (bad) return { ok: false, reason: bad };
  const picked = await pick(io, `citrate-device-keys-${day(now)}.${FILE_EXT}`, FILE_FILTER);
  if (typeof picked !== "string") return picked;
  const path = filePath(picked);
  try {
    const keys = await io.invoke<number>("recovery_kit_save_file", { path, passphrase });
    return { ok: true, path, keys };
  } catch (e) {
    return { ok: false, reason: messageOf(e) };
  }
}

/** Restore from the member's typed or pasted phrase sheet. */
export async function restoreFromPhrase(io: PrivacyIo, sheet: string, replace: boolean): Promise<RestoreResult> {
  if (io.mode !== "tauri") return { ok: false, reason: DESKTOP_ONLY };
  if (!sheet.trim()) return { ok: false, reason: "Type or paste your recovery phrase sheet first." };
  try {
    const r = await io.invoke<RestoreReport>("recovery_kit_restore_phrase", { sheet, replace });
    return { ok: true, ...r };
  } catch (e) {
    return { ok: false, reason: messageOf(e) };
  }
}

/** Restore from a sealed recovery file the member picks. */
export async function restoreFromFile(io: PrivacyIo, passphrase: string, replace: boolean): Promise<RestoreResult> {
  if (io.mode !== "tauri") return { ok: false, reason: DESKTOP_ONLY };
  if (!passphrase) return { ok: false, reason: "Enter the passphrase the recovery file was made with." };
  let path: string | null;
  try {
    path = await io.pickOpenPath(FILE_FILTER);
  } catch (e) {
    return { ok: false, reason: "Could not open the file dialog: " + messageOf(e) };
  }
  if (!path) return { ok: false, cancelled: true, reason: "Cancelled." };
  try {
    const r = await io.invoke<RestoreReport>("recovery_kit_restore_file", { path, passphrase, replace });
    return { ok: true, ...r };
  } catch (e) {
    return { ok: false, reason: messageOf(e) };
  }
}
