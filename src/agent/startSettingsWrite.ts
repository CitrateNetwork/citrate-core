// One write of a setting Hermes reads at start (the saved web settings, and learning from
// sessions), as every writer must do it: inside the shared write lock (webSettingsSync.ts), and not
// while Hermes is starting (shell/slices/hermesRunning.ts).
//
// The start check runs again once the lock is held: a write that was queued behind another one
// when a start began is held, not run. A start, for its part, takes its snapshot inside the same
// lock (withHermesStart), so it waits for a write already in flight and never sees half of one:
// what Hermes reads and what the app records as running are the same value.
// A tripwire (shell/slices/hermesStartInFlightWrite.test.tsx) fails on a writer that skips this.
import { withWebSettingsLock } from "./webSettingsSync";
import { hermesStarting } from "../shell/slices/hermesRunning";

/** Run `write` under the lock, unless Hermes is starting. Resolves to null when the write was held. */
export function writeStartSettingUnlessStarting<T>(write: () => Promise<T>): Promise<T | null> {
  return withWebSettingsLock(async () => (hermesStarting() ? null : write()));
}
