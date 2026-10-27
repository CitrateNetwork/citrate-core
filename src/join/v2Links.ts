// core#423 — v2 invite links (the citrate.ai/join short links and public links, GROW-S1 tasks 11-12).
//
// Owner decision 2026-10-27: hidden in 0.5.2. The app pins no resolve-signing key yet
// (`JOIN_PINNED_KEYS = &[]` in kit/src/join.rs), so a v2 link could never be opened, and D-6 says a
// missing key ships an honest "not available". One build-time switch turns them on:
// `VITE_V2_INVITE_LINKS=1`. A production build refuses that switch while no key is pinned
// (v2LinksBuildGuard.ts, run from vite.config.ts), and Rust refuses to make a v2 link without one.
export function v2InviteLinksEnabled(): boolean {
  return import.meta.env.VITE_V2_INVITE_LINKS === "1";
}

/** What the member sees where a v2 invite link control would be. */
export const V2_LINKS_NOT_AVAILABLE = "Short invite links aren't available in this version. To add someone, use Invite by @handle in Groups.";

/** The toast for a v2 link or code that arrives while v2 links are off. */
export const V2_LINK_NOT_ACCEPTED = "Short invite links (citrate.ai/join) aren't available in this version. Ask for an app invite link instead.";
