// =====================================================================
// citrate-core — structured group messages (0.5.4 FL design, board 02).
//
// Group chat rides the comms-member-daemon's MLS group messages: each one is (sender, body), and the
// sender is the address the daemon authenticated. Two kinds of message carry structure, and both are
// written as plain text a member can read in any app version, so a 0.5.3 member (whose app shows
// every body as text) sees readable words, never JSON. The structure is the LAST line, which starts
// with U+0001 (invisible in older apps, and nothing a member types can produce it):
//
//   reply    "<text>" then "\u0001(in reply to m1a2b3c4d)"
//   Hermes   "Hermes, on this member's device, answered because they mentioned it:" then the answer,
//            an optional round draft ("Round draft: <title> (no SALT moved)" and one line of values),
//            "What Hermes read: a; b; c." and "\u0001(Hermes answer to m1a2b3c4d)".
//
// The device a Hermes answer ran on is the authenticated SENDER, never anything in the body, so
// nobody can post a Hermes answer "from" someone else's device.
//
// A message's ref is a short FNV-1a hash of its sender and body. The daemon gives no message id, so
// the ref is what a reply points at; two identical messages from one sender share a ref, and a
// reply then threads under the first. Every field is checked here; a malformed body is shown as
// plain text (marker removed), never dropped silently, and a body that STARTS with U+0001 (a control
// message, or one from a newer app) is hidden like the existing control messages.
// =====================================================================
import type { GroupMember, GroupMessage } from "../../bridge/domains";
import { isControlMessage } from "../groupControlMessages";

/** The first line of a Hermes answer, as every app version shows it. */
export const HERMES_HEADER = "Hermes, on this member's device, answered because they mentioned it:";
const REPLY_MARK = /\n\u0001\(in reply to (m[0-9a-f]{8})\)$/;
const HERMES_MARK = /\n\u0001\(Hermes answer to (m[0-9a-f]{8})\)$/;
const ANY_MARK = /\n\u0001\((in reply to|Hermes answer to) [^\n]*\)$/;

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

const clip = (s: string) => s.slice(0, MAX_TEXT);
/** A member's or the model's text: control characters removed (tab and newline kept), so it can
 *  never carry a structure line of its own. */
const plain = (s: string) => s.replace(/[\u0000-\u0008\u000b-\u001f\u007f]/g, "");
/** One line, no sentinel: what goes inside a fixed-format line. */
const line = (s: string) => s.replace(/[\u0000-\u001f]+/g, " ").trim();

export function encodeReply(to: string, text: string): string {
  return `${clip(plain(text).trim())}\n\u0001(in reply to ${to})`;
}

const fmt = (n: number) => n.toLocaleString("en-US", { maximumFractionDigits: 4, useGrouping: false });

/** The lines a round draft adds to a posted Hermes answer (shown on the send card verbatim). */
export function draftLines(d: RoundDraft): string[] {
  const v = (x: number | null, unit: string) => (x === null ? "not proposed" : `${fmt(x)}${unit}`);
  return [
    `Round draft: ${line(d.title)} (no SALT moved)`,
    `Budget: ${v(d.budgetSalt, " SALT")}. Rate: ${v(d.rateSalt, " SALT per lease")}. Workers: ${d.workers === null ? "not proposed" : "up to " + d.workers}. Deadline: ${d.deadline ?? "not proposed"}.`,
  ];
}

export function encodeHermes(e: HermesEnvelope): string {
  const parts = [HERMES_HEADER, clip(plain(e.text).trim())];
  if (e.draft) parts.push("", ...draftLines(e.draft));
  const read = e.read.slice(0, 8).map((r) => line(r).replace(/;/g, ",").replace(/\.$/, ""));
  parts.push("", `What Hermes read: ${read.join("; ")}.`, `\u0001(Hermes answer to ${e.to})`);
  return parts.join("\n");
}

const num = (v: unknown, max: number): number | null =>
  typeof v === "number" && Number.isFinite(v) && v >= 0 && v <= max ? v : null;

/** A draft from untrusted input: every field checked, absurd numbers dropped to null. */
export function checkDraft(raw: unknown): RoundDraft | null {
  if (!raw || typeof raw !== "object") return null;
  const d = raw as Record<string, unknown>;
  const title = typeof d.title === "string" ? line(d.title).slice(0, 160) : "";
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

/** A structured reply, or null when the body is not one. */
export function decodeReply(body: string): { to: string; text: string } | null {
  const m = body.match(REPLY_MARK);
  if (!m) return null;
  return { to: m[1], text: clip(body.slice(0, m.index).trim()) };
}

const DRAFT_TITLE = /^Round draft: (.+) \(no SALT moved\)$/;
const DRAFT_VALUES = /^Budget: (not proposed|([\d.]+) SALT)\. Rate: (not proposed|([\d.]+) SALT per lease)\. Workers: (not proposed|up to (\d+))\. Deadline: (not proposed|\d{4}-\d{2}-\d{2})\.$/;

/** A Hermes answer, or null when the body is not one (or its header or read line is missing). */
export function decodeHermes(body: string): HermesEnvelope | null {
  const m = body.match(HERMES_MARK);
  if (!m) return null;
  const lines = body.slice(0, m.index).split("\n");
  if (lines[0] !== HERMES_HEADER) return null;
  const last = lines[lines.length - 1] ?? "";
  if (!last.startsWith("What Hermes read: ")) return null;
  const read = last
    .slice("What Hermes read: ".length)
    .replace(/\.$/, "")
    .split("; ")
    .map((r) => r.trim())
    .filter(Boolean)
    .slice(0, 8);
  let end = lines.length - 1;
  let draft: RoundDraft | null = null;
  const t = lines[end - 3]?.match(DRAFT_TITLE);
  const v = lines[end - 2]?.match(DRAFT_VALUES);
  if (t && v && lines[end - 1] === "") {
    draft = checkDraft({
      title: t[1],
      budgetSalt: v[2] !== undefined ? Number(v[2]) : null,
      rateSalt: v[4] !== undefined ? Number(v[4]) : null,
      workers: v[6] !== undefined ? Number(v[6]) : null,
      deadline: v[7] === "not proposed" ? null : v[7],
    });
    end -= 3;
  }
  const text = lines.slice(1, end).join("\n").trim();
  return { to: m[1], text: clip(text), read, draft };
}

/** A body with a structure line that did not parse: the words, marker removed. */
const strayMarkText = (body: string): string | null => (ANY_MARK.test(body) ? body.replace(ANY_MARK, "").trim() : null);

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
    const ref = messageRef(m.sender, body);
    const reply = decodeReply(body);
    if (reply) {
      const parent = byRef.get(reply.to);
      if (parent) {
        parent.replies.unshift({ ref, sender: m.sender, text: reply.text });
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
      : { kind: "message", ref, sender: m.sender, text: strayMarkText(body) ?? body, ts: m.ts, replies: [] };
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
    if (isControlMessage(body) || body.startsWith("\u0001")) continue;
    const h = decodeHermes(body);
    const text = h ? "Hermes: " + h.text : decodeReply(body)?.text ?? strayMarkText(body) ?? body;
    if (text) return text.replace(/\s+/g, " ").slice(0, 80);
  }
  return "";
}
