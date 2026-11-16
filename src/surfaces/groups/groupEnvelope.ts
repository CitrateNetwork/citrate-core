// =====================================================================
// citrate-core — structured group messages (0.5.4 FL design, board 02).
//
// Group chat rides the comms-member-daemon's MLS group messages: each one is (sender, body), and the
// sender is the address the daemon authenticated. Two kinds of message carry structure in the body,
// behind a U+0001 prefix like the existing control messages (nothing a member types can match it):
//
//   creply1   a reply in a thread: { v: 1, to: <ref of the parent>, text }
//   chermes1  a Hermes answer a member's own device produced and posted: { v: 1, to, text, read,
//             draft? }. The device it ran on is the authenticated SENDER, never a field in the body,
//             so nobody can post a Hermes answer "from" someone else's device.
//
// A message's ref is a short FNV-1a hash of its sender and body. The daemon gives no message id, so
// the ref is what a reply points at; two identical messages from one sender share a ref, and a
// reply then threads under the first. Every field is checked here; a malformed body is shown as
// plain text, never dropped silently, and an unknown U+0001 body (a control message from a newer
// app) is hidden like the existing control messages.
// =====================================================================
import type { GroupMember, GroupMessage } from "../../bridge/domains";
import { isControlMessage } from "../groupControlMessages";

export const REPLY_MSG_PREFIX = "\u0001creply1:";
export const HERMES_MSG_PREFIX = "\u0001chermes1:";

/** Longest text a structured message may carry (the composer and Hermes are cut to this). */
export const MAX_TEXT = 4000;

/** A round draft Hermes proposed. Numbers are Hermes's proposal, shown as such, never as a reading. */
export interface RoundDraft {
  title: string;
  /** Proposed budget in whole SALT, held in escrow only when an owner or admin opens the round. */
  budgetSalt: number | null;
  /** Proposed rate per lease in SALT. */
  rateSalt: number | null;
  /** Proposed most workers. */
  workers: number | null;
  /** Proposed deadline, YYYY-MM-DD. */
  deadline: string | null;
}

export interface HermesEnvelope {
  to: string;
  text: string;
  read: string[];
  draft: RoundDraft | null;
}

export type ThreadItem =
  | { kind: "message"; ref: string; sender: string; text: string; ts: number; replies: ReplyItem[] }
  | { kind: "hermes"; ref: string; device: string; to: string; text: string; read: string[]; draft: RoundDraft | null; ts: number; replies: ReplyItem[] };

export interface ReplyItem {
  ref: string;
  sender: string;
  text: string;
}

/** FNV-1a 32-bit over the UTF-16 code units, as 8 hex digits. Not a security hash: a thread link. */
function fnv1a(s: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, "0");
}

/** The ref a reply uses to point at a message. */
export function messageRef(sender: string, body: string): string {
  // The same address can arrive with or without 0x (the cluster daemon drops it): normalize both.
  return "m" + fnv1a(sender.replace(/^0x/i, "").toLowerCase() + "\n" + body);
}

const REF = /^m[0-9a-f]{8}$/;
const clip = (s: string) => s.slice(0, MAX_TEXT);

export function encodeReply(to: string, text: string): string {
  return REPLY_MSG_PREFIX + JSON.stringify({ v: 1, to, text: clip(text) });
}

export function encodeHermes(e: HermesEnvelope): string {
  return HERMES_MSG_PREFIX + JSON.stringify({ v: 1, to: e.to, text: clip(e.text), read: e.read.slice(0, 8), ...(e.draft ? { draft: e.draft } : {}) });
}

const num = (v: unknown, max: number): number | null =>
  typeof v === "number" && Number.isFinite(v) && v >= 0 && v <= max ? v : null;

/** A draft from untrusted JSON: every field checked, absurd numbers dropped to null. */
export function checkDraft(raw: unknown): RoundDraft | null {
  if (!raw || typeof raw !== "object") return null;
  const d = raw as Record<string, unknown>;
  const title = typeof d.title === "string" ? d.title.trim().slice(0, 160) : "";
  if (!title) return null;
  const deadline = typeof d.deadline === "string" && /^\d{4}-\d{2}-\d{2}$/.test(d.deadline) ? d.deadline : null;
  const workers = num(d.workers, 10_000);
  return {
    title,
    budgetSalt: num(d.budgetSalt, 1e12),
    rateSalt: num(d.rateSalt, 1e9),
    workers: workers === null ? null : Math.floor(workers),
    deadline,
  };
}

function parseJson(body: string, prefix: string): Record<string, unknown> | null {
  try {
    const v = JSON.parse(body.slice(prefix.length));
    return v && typeof v === "object" && v.v === 1 ? (v as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

/** A structured reply, or null when the body is not one (or is malformed). */
export function decodeReply(body: string): { to: string; text: string } | null {
  if (!body.startsWith(REPLY_MSG_PREFIX)) return null;
  const v = parseJson(body, REPLY_MSG_PREFIX);
  if (!v || typeof v.to !== "string" || !REF.test(v.to) || typeof v.text !== "string") return null;
  return { to: v.to, text: clip(v.text) };
}

/** A Hermes answer, or null when the body is not one (or is malformed). */
export function decodeHermes(body: string): HermesEnvelope | null {
  if (!body.startsWith(HERMES_MSG_PREFIX)) return null;
  const v = parseJson(body, HERMES_MSG_PREFIX);
  if (!v || typeof v.to !== "string" || !REF.test(v.to) || typeof v.text !== "string") return null;
  const read = Array.isArray(v.read) ? v.read.filter((x): x is string => typeof x === "string").map((x) => x.slice(0, 120)).slice(0, 8) : [];
  return { to: v.to, text: clip(v.text), read, draft: checkDraft(v.draft) };
}

/** Is this body one of the structured kinds above (shown), or another U+0001 control message (hidden)? */
const isStructured = (body: string) => body.startsWith(REPLY_MSG_PREFIX) || body.startsWith(HERMES_MSG_PREFIX);

/**
 * The thread a member reads, oldest first: plain messages and Hermes answers at the top level, each
 * with the replies that point at it. A reply whose parent is not in the retained history is shown
 * at the top level, so nothing a member sent goes missing. Control messages never render.
 */
export function threadModel(messages: GroupMessage[]): ThreadItem[] {
  const items: ThreadItem[] = [];
  const byRef = new Map<string, ThreadItem>();
  for (const m of messages) {
    const body = m.body;
    if (isControlMessage(body)) continue;
    if (body.startsWith("\u0001") && !isStructured(body)) continue;
    const ref = messageRef(m.sender, body);
    const reply = decodeReply(body);
    if (reply) {
      const parent = byRef.get(reply.to);
      if (parent) {
        parent.replies.push({ ref, sender: m.sender, text: reply.text });
        continue;
      }
      const item: ThreadItem = { kind: "message", ref, sender: m.sender, text: reply.text, ts: m.ts, replies: [] };
      items.push(item);
      byRef.set(ref, item);
      continue;
    }
    const h = decodeHermes(body);
    const item: ThreadItem = h
      ? { kind: "hermes", ref, device: m.sender, to: h.to, text: h.text, read: h.read, draft: h.draft, ts: m.ts, replies: [] }
      : { kind: "message", ref, sender: m.sender, text: isStructured(body) ? "(a message this app could not read)" : body, ts: m.ts, replies: [] };
    if (!byRef.has(ref)) byRef.set(ref, item);
    items.push(item);
  }
  return items;
}

/** The person a top-level item came from (for Hermes: the member whose device ran it). */
export const itemSender = (i: ThreadItem): string => (i.kind === "hermes" ? i.device : i.sender);

/** A member's role in the roster, or null. */
export function roleOf(roster: GroupMember[], address: string): GroupMember["role"] | null {
  return roster.find((r) => r.address.toLowerCase() === address.toLowerCase())?.role ?? null;
}

/** A one-line preview of the last thing said in a group (for the group list). */
export function lastLine(messages: GroupMessage[]): string {
  for (let i = messages.length - 1; i >= 0; i--) {
    const body = messages[i].body;
    if (isControlMessage(body) || (body.startsWith("\u0001") && !isStructured(body))) continue;
    const h = decodeHermes(body);
    const text = h ? "Hermes: " + h.text : decodeReply(body)?.text ?? (isStructured(body) ? "" : body);
    if (text) return text.replace(/\s+/g, " ").slice(0, 80);
  }
  return "";
}
