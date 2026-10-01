// =====================================================================
// HUP-S10.5 — "delete my local data" (webview half).
//
// The dry run comes first and lists every folder item and keychain entry with
// what will happen to it. The delete needs the typed phrase (and a second one to
// include the wallet). The webview clears its own storage, then Rust stops the
// sidecars, deletes the plan it rebuilds itself, and closes the app.
// =====================================================================
import { DESKTOP_ONLY, messageOf, type PrivacyIo } from "./privacyIo";

export const CONFIRM_PHRASE = "delete my local data";
export const WALLET_CONFIRM_PHRASE = "delete my wallet";

export interface DeleteOptions {
  includeWallet: boolean;
  keepModels: boolean;
}
export type Action = "delete" | "keep";
export interface PlanEntry {
  path: string;
  kind: "data" | "cache" | "logs" | "webview";
  bytes: number;
  action: Action;
  reason: string | null;
}
export interface KeychainEntry {
  service: string;
  account: string;
  label: string;
  present: boolean | null;
  action: Action;
}
export interface DataPlan {
  options: DeleteOptions;
  entries: PlanEntry[];
  keychain: KeychainEntry[];
  deleteBytes: number;
  keepBytes: number;
  confirmPhrase: string;
  walletConfirmPhrase: string | null;
  notes: string[];
}
export interface Failure {
  item: string;
  error: string;
}
export interface DeleteReport {
  deleted: string[];
  failed: Failure[];
  keychainDeleted: string[];
  keychainFailed: Failure[];
}

type Fail = { ok: false; reason: string };

const norm = (s: string) => s.trim().split(/\s+/).join(" ").toLowerCase();

/** The confirmation rule, mirrored from Rust (which checks again). */
export function confirmationError(opts: DeleteOptions, confirm: string, walletConfirm: string): string | null {
  if (norm(confirm) !== CONFIRM_PHRASE) return `Type "${CONFIRM_PHRASE}" to confirm.`;
  if (opts.includeWallet && norm(walletConfirm) !== WALLET_CONFIRM_PHRASE) {
    return `To also delete your wallet, type "${WALLET_CONFIRM_PHRASE}" in the second box.`;
  }
  return null;
}

export async function planLocalData(io: PrivacyIo, options: DeleteOptions): Promise<{ ok: true; plan: DataPlan } | Fail> {
  if (io.mode !== "tauri") return { ok: false, reason: DESKTOP_ONLY };
  try {
    return { ok: true, plan: await io.invoke<DataPlan>("local_data_plan", { options }) };
  } catch (e) {
    return { ok: false, reason: messageOf(e) };
  }
}

export async function deleteLocalData(
  io: PrivacyIo,
  options: DeleteOptions,
  confirm: string,
  walletConfirm: string,
): Promise<{ ok: true; report: DeleteReport } | Fail> {
  if (io.mode !== "tauri") return { ok: false, reason: DESKTOP_ONLY };
  const bad = confirmationError(options, confirm, walletConfirm);
  if (bad) return { ok: false, reason: bad };
  io.clearWebStorage();
  try {
    const report = await io.invoke<DeleteReport>("local_data_delete", {
      options,
      confirm,
      walletConfirm: options.includeWallet ? walletConfirm : null,
    });
    return { ok: true, report };
  } catch (e) {
    return { ok: false, reason: messageOf(e) };
  }
}

/** Bytes as a file manager shows them (binary units). */
export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(1)} ${units[i]}`;
}
