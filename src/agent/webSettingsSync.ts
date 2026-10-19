// One source of truth for the saved web settings while more than one view shows them.
//
// Hermes › Controls mounts two writers of hermes_web_settings: the Controls panel's Web search
// switch and the Settings › Web search & decisions form. Each re-reads before it writes and changes
// only its own fields; this module closes the remaining gap, the display. After a writer saves, it
// publishes what core answered; every other mounted view adopts it, so none keeps showing (or
// diffing against) a snapshot that is no longer what core has saved.
//
// In-memory and synchronous: nothing persists here, core is still the only store.
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
