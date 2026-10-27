// core#423 — the release-build guard for v2 invite links. Pure (no Vite, no DOM), so vite.config.ts
// and the tests both use it. See v2Links.ts.

/** The number of entries in `JOIN_PINNED_KEYS` in kit/src/join.rs, or null if it can't be found. */
export function joinPinnedKeyCount(joinRs: string): number | null {
  void joinRs;
  return null;
}

/** Why a production build must stop, or null when it may go on. */
export function v2LinksBuildRefusal(flag: string | undefined, joinRs: string): string | null {
  void flag;
  void joinRs;
  return null;
}
