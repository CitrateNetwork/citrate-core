// One write of the saved web settings, as every writer must do it: inside the shared write lock
// (webSettingsSync.ts), and not while Hermes is starting (shell/slices/hermesRunning.ts), because
// Hermes reads these settings at start.
//
// The start check runs again once the lock is held: a write that was queued behind another one
// when a start began is held, not run. A start, for its part, takes its snapshot inside the same
// lock (withHermesStart), so it waits for a write already in progress and never sees half of one.
import { withWebSettingsLock } from "./webSettingsSync";
import { hermesStarting } from "../shell/slices/hermesRunning";

/** Run `write` under the lock, unless Hermes is starting. Resolves to null when the write was held. */
export function writeWebSettingsUnlessStarting<T>(write: () => Promise<T>): Promise<T | null> {
  return withWebSettingsLock(async () => (hermesStarting() ? null : write()));
}
