// D-12 — the one invite link the app understands: `citrate://invite?…` (request and approve). This
// only tells whether a pasted or deep-linked string is one; Rust reads and checks it
// (`group_invite_preview`), so a malformed link gets Rust's error, never a guess made here.

/** The longest link the app passes on (Rust refuses longer ones too). */
export const MAX_INVITE_LINK = 2048;

/** Whether `raw` is an invite link (by its scheme; the rest is checked in Rust). */
export function isInviteLink(raw: string): boolean {
  const s = (raw || "").trim();
  return s.length > 0 && s.length <= MAX_INVITE_LINK && /^citrate:\/\/invite\?/i.test(s);
}

/** "expires 9 Nov 2026" for a unix-seconds expiry. */
export function expiryText(expiresAt: number): string {
  return "expires " + new Date(expiresAt * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

/** A short rendering of an address: 0x1234…abcd. */
export function shortAddress(a: string): string {
  return a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a || "—";
}
