// One write of a setting Hermes reads at start (the saved web settings, and learning from
// sessions), as every writer must do it: inside the shared write lock (webSettingsSync.ts), and not
// while Hermes is starting (shell/slices/hermesRunning.ts).
//
// The start check runs again once the lock is held: a write that was queued behind another one
// when a start began is held, not run. A start, for its part, takes its snapshot inside the same
// lock (withHermesStart), so it waits for a write already in flight and never sees half of one:
// what Hermes reads and what the app records as running are the same value.
// A tripwire (shell/slices/hermesStartInFlightWrite.test.tsx) fails on a writer that skips this.
import { publishWebSettings, withWebSettingsLock } from "./webSettingsSync";
import { loadWebSettings, saveWebSettings, type HermesWebSettings, type HermesWebStatus, type WebSettingsIo } from "./webSearch";
import { hermesStarting, noteRunning } from "../shell/slices/hermesRunning";

/** Run `write` under the lock, unless Hermes is starting. Resolves to null when the write was held. */
export function writeStartSettingUnlessStarting<T>(write: () => Promise<T>): Promise<T | null> {
  return withWebSettingsLock(async () => (hermesStarting() ? null : write()));
}

/**
 * The one writer of the saved web settings, inside the lock and the start hold above: re-read what core has, change only `fields`, write
 * the whole object back (core replaces it on every set), and tell the other views.
 * Resolves null when the write was held for a Hermes start.
 */
export function patchWebSettings(io: WebSettingsIo, fields: Partial<HermesWebSettings>, source: symbol): Promise<HermesWebStatus | null> {
  return writeStartSettingUnlessStarting(async () => {
    const fresh = await loadWebSettings(io);
    // Hermes reads search at start: remember what it runs with before changing the saved value.
    noteRunning("webSearch", fresh.settings.searchEnabled);
    const saved = await saveWebSettings(io, { ...fresh.settings, ...fields });
    publishWebSettings(saved, source);
    return saved;
  });
}
