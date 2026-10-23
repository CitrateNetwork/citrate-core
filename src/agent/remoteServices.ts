// =====================================================================
// K2 (owner decision 2026-10-19): "Local model only" covers chat and escalation. Owner decision
// 2026-10-23: it ALSO covers image generation through a provider: no prompt leaves the machine in
// that mode; an image needs the local route or is refused with the reason (core enforces it,
// `media.rs`). TypeSafe Jev is still not blocked by it, so it says plainly that it uses a remote
// service and, while Local only is on, that Local only does not apply to it.
// =====================================================================

/** "Uses a remote service (<provider>)." */
export function remoteServiceLine(provider: string): string {
  return `Uses a remote service (${provider}).`;
}

/** The media panel's note while Local model only is on (owner decision 2026-10-23). */
export const LOCAL_ONLY_MEDIA_NOTE =
  "Local model only is on: images are made only on this device, and no prompt is sent to a provider.";

/** The note shown while Local model only is on, for a remote service it does not cover. */
export function localOnlyDoesNotApply(what: string): string {
  return `Local model only does not apply to ${what}: it covers chat, escalation and image generation.`;
}
