// GROW-S1 — wire the OS `citrate://` deep-link into the app (the one-tap cold-start hand-off from the
// web join page). Two cases: the app was LAUNCHED by the link (getCurrent) and the app was ALREADY
// running when the link fired (onOpenUrl). Both route to store.handleDeepLink → parseJoinLink → Groups.
// Tauri-only; a no-op (and never throws) in the web/sim preview where the plugin isn't present.
import type { Store } from "./store";
import { BRIDGE_MODE } from "../bridge/mode";

export async function wireDeepLinks(store: Store): Promise<void> {
  if (BRIDGE_MODE !== "tauri") return; // no OS deep-links in the web preview
  try {
    const { onOpenUrl, getCurrent } = await import("@tauri-apps/plugin-deep-link");
    // Cold start: the link that launched the app (if any).
    const launched = await getCurrent().catch(() => null);
    if (launched && launched.length) store.handleDeepLink(launched[0]);
    // Warm: links delivered while the app is already open.
    await onOpenUrl((urls) => {
      if (urls && urls.length) store.handleDeepLink(urls[0]);
    });
  } catch {
    /* plugin unavailable (older build / not tauri) — deep-links simply don't fire; no crash */
  }
}
