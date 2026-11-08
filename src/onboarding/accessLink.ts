// Download gate: the access link in the download email, `citrate://access?code=<code>`. This only
// tells whether a string is one; Rust reads and checks it (`access_pass_verify`). Not a group invite
// (those are `citrate://invite?…`, D-12).

/** The longest link the app passes on (Rust refuses longer input too). */
export const MAX_ACCESS_LINK = 512;

/** Whether `raw` is an access link (by its scheme and host; the code is checked in Rust). */
export function isAccessLink(raw: string): boolean {
  const s = (raw || "").trim();
  return s.length > 0 && s.length <= MAX_ACCESS_LINK && /^citrate:\/\/access\?/i.test(s);
}
