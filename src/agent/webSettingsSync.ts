// One source of truth for the saved web settings while more than one view shows them.
//
// Hermes › Controls mounts two writers of hermes_web_settings: the Controls panel's Web search
// switch and the Settings › Web search & decisions form. Each re-reads before it writes and changes
// only its own fields; this module closes the remaining gap, the display. After a writer saves, it
// publishes what core answered; every other mounted view adopts it, so none keeps showing (or
// diffing against) a snapshot that is no longer what core has saved.
//
// Writes are serialized: each writer runs its re-read, field change, write and announcement inside
// `withWebSettingsLock`, so two writers can never both read before either writes (which would let
// the second write put back a field the first had just changed).
//
// In-memory: nothing persists here, core is still the only store.
import type { HermesWebStatus } from "./webSearch";

export type WebSettingsListener = (status: HermesWebStatus, source: symbol) => void;

const listeners = new Set<WebSettingsListener>();

/** Tell every other mounted view what core saved. `source` identifies the writer, which ignores its own event. */
export function publishWebSettings(status: HermesWebStatus, source: symbol): void {
  for (const l of [...listeners]) l(status, source);
}

/** Listen for saves made by other views. Returns the unsubscribe. */
export function subscribeWebSettings(listener: WebSettingsListener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

// The tail of the write queue. Each write waits for the previous one to settle (fulfilled or not).
let tail: Promise<unknown> = Promise.resolve();

/**
 * Run one read-modify-write of the saved web settings with no other write in between. Writes run
 * in the order they were asked for. A failed write releases the lock and rejects to its caller.
 */
export function withWebSettingsLock<T>(write: () => Promise<T>): Promise<T> {
  const run = tail.then(write, write);
  tail = run.catch(() => undefined);
  return run;
}
