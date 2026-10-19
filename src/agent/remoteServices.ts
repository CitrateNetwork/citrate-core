// =====================================================================
// K2 (owner decision 2026-10-19): "Local model only" covers chat and escalation only. Image
// generation through a provider and TypeSafe Jev are not blocked by it, so where they are used
// they say plainly that they use a remote service, and, while Local only is on, that it does not
// apply to them.
// =====================================================================

/** "Uses a remote service (<provider>)." */
export function remoteServiceLine(provider: string): string {
  return `Uses a remote service (${provider}).`;
}

/** The note shown while Local model only is on. */
export function localOnlyDoesNotApply(what: string): string {
  return `Local model only does not apply to ${what}: it covers chat and escalation only.`;
}
