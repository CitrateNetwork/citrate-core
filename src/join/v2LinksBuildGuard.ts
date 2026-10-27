// core#423 — the release-build guard for v2 invite links. Pure (no Vite, no DOM), so vite.config.ts
// and the tests both use it. See v2Links.ts.

/** The number of entries in `JOIN_PINNED_KEYS` in kit/src/join.rs, or null if it can't be found.
 *  Each entry is one `("kid", "x")` tuple; comment lines are ignored. */
export function joinPinnedKeyCount(joinRs: string): number | null {
  const m = /pub const JOIN_PINNED_KEYS: &\[\(&str, &str\)\] = &\[([^\]]*)\];/.exec(joinRs);
  if (!m) return null;
  const body = m[1]
    .split("\n")
    .filter((l) => !l.trim().startsWith("//"))
    .join("\n");
  return (body.match(/\(\s*"[^"]*"\s*,\s*"[^"]*"\s*\)/g) ?? []).length;
}

/** Why a production build must stop, or null when it may go on. `flag` is VITE_V2_INVITE_LINKS;
 *  `joinRs` is the text of kit/src/join.rs. Pins that can't be read count as none (fails closed). */
export function v2LinksBuildRefusal(flag: string | undefined, joinRs: string): string | null {
  if (flag !== "1") return null;
  if ((joinPinnedKeyCount(joinRs) ?? 0) > 0) return null;
  return (
    "VITE_V2_INVITE_LINKS=1 turns v2 invite links on, but kit/src/join.rs JOIN_PINNED_KEYS pins no " +
    "resolve-signing key, so no v2 link could be opened. Pin the keys first, or build without the " +
    "switch (core#423)."
  );
}
