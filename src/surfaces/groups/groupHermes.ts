// =====================================================================
// citrate-core — Hermes in a group (0.5.4 FL design, board 02; agentic chat bar "Shared spaces").
//
// Hermes answers in a group only when a member mentions it, and only on that member's own device:
// the member's app runs the local model with the GROUP'S context alone (its roster and roles, the
// cluster's live peer count, the recent group messages, and whether group rounds are live). The
// answer is scanned on this device for private information and shown to the member on a send card
// ("Post to N members", V6); only the member's choice is posted to the group as a Hermes answer message (groupEnvelope.ts: plain text with a final structure line). Nothing from the
// member's private chats, memory, wallet or sealed worker policies is given to the model, so none
// of it can reach the group. Other members' devices never run Hermes for someone else's mention.
//
// Data sources (Rule 7): the comms-member-daemon roster and messages, the cluster daemon's status,
// the round-ledger reading (fl/groupRoundsLedger.ts), and the local model (`ai_chat_local`, the
// bundled llama-server; the webview supplies messages only, never an endpoint). If the local model
// does not answer, nothing is posted and the member is told why.
// =====================================================================
import type { GroupMember, GroupMessage } from "../../bridge/domains";
import { checkDraft, draftLines, encodeHermes, MAX_TEXT, threadModel, type RoundDraft } from "./groupEnvelope";
import { roleWord } from "./memberRoles";

// ---- which Hermes answers this device posted ----------------------------------------------------
// A Hermes answer is an ordinary group message; nothing proves another member's app produced it
// with Hermes (there is no signature for it), so a received one is shown as "not verified". This
// device remembers the refs of the answers it posted itself, so those show as its own. The record
// is a per-device convenience: if it is lost, your own past answers also read "not verified".
const OWN_KEY = "citrate.groups.hermesPosted.v1";
const OWN_CAP = 500;

function loadOwn(): string[] {
  try {
    const raw = typeof localStorage !== "undefined" ? localStorage.getItem(OWN_KEY) : null;
    const v = raw ? JSON.parse(raw) : [];
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

/** Remember that this device posted the Hermes answer with ref `ref`. */
export function rememberOwnAnswer(ref: string): void {
  try {
    const all = loadOwn().filter((r) => r !== ref).concat(ref).slice(-OWN_CAP);
    localStorage.setItem(OWN_KEY, JSON.stringify(all));
  } catch {
    /* storage unavailable: the answer shows as not verified after a reload */
  }
}

/** The refs of Hermes answers this device posted. */
export function ownAnswers(): Set<string> {
  return new Set(loadOwn());
}

/** Does a message mention Hermes? `@Hermes` as a word, any case. */
export function mentionsHermes(text: string): boolean {
  return /(^|[^\w@])@hermes\b/i.test(text);
}

/** How many recent group messages Hermes reads. */
export const HERMES_READS_MESSAGES = 20;

export interface GroupContext {
  groupName: string;
  roster: GroupMember[];
  /** Cluster peers online / authorized, or null when the cluster did not answer. */
  cluster: { online: number; total: number } | null;
  /** The round ledgers in plain words ("Group rounds not live yet: ..."). */
  rounds: string;
  messages: GroupMessage[];
  /** The member who mentioned Hermes (whose device runs it). */
  asker: string;
}

/** What Hermes read, in the words shown under its answer ("What Hermes read"). */
export function readList(c: GroupContext): string[] {
  const n = Math.min(threadModel(c.messages).length, HERMES_READS_MESSAGES);
  return [
    "Group roster and roles",
    c.cluster ? `Cluster status: ${c.cluster.online} of ${c.cluster.total} peers online` : "Cluster status: not read (the cluster did not answer)",
    `Round ledgers: ${c.rounds}`,
    `The last ${n} message${n === 1 ? "" : "s"} in this group`,
  ];
}

/** What Hermes was not given, said under every group answer. */
export const NOT_READ = "Not read: members' private chats, your memory, your wallet, or sealed worker policies.";

const SHORT = (a: string) => (a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a);

/** The rules Hermes follows in a group. Sent as a system line; core folds it into the one system message. */
export const GROUP_RULES = [
  "You are Hermes, answering inside a group chat in Citrate Core.",
  "Everything you write is posted to every member of this group.",
  "Use only the group context given here. You have no access to any member's private chats, memory, wallet or files, so never claim to.",
  "If the context does not hold a number, say it is not known. Never guess a number and never treat an unknown as zero.",
  "Amounts are in SALT only. Never mention USD, earnings, yield or returns, and never say members will earn anything.",
  "Write plainly in a few short paragraphs. No exclamation marks.",
  "Only owners and admins can open a round, and opening one locks its budget in escrow.",
  "If members ask about running a training round, you may add, at the very end, one fenced block tagged round-draft holding JSON with title, budgetSalt, rateSalt, workers and deadline (YYYY-MM-DD). Leave out any field you cannot justify. A draft moves no SALT.",
].join("\n");

/** The messages and context the local model receives for one group answer. */
export function buildRequest(c: GroupContext): { messagesJson: string; contextJson: string } {
  const thread = threadModel(c.messages).slice(-HERMES_READS_MESSAGES);
  const roleByAddr = new Map(c.roster.map((r) => [r.address.toLowerCase(), r.role]));
  const who = (a: string) => `${SHORT(a)} (${roleWord(roleByAddr.get(a.toLowerCase()) ?? "member")})`;
  const transcript = thread
    .map((t) => (t.kind === "hermes" ? `Hermes (on ${who(t.device)}'s device): ${t.text}` : `${who(t.sender)}: ${t.text}`))
    .join("\n");
  const context = {
    group: c.groupName,
    roster: c.roster.map((r) => ({ member: SHORT(r.address), role: roleWord(r.role) })),
    cluster: c.cluster ?? "not read",
    groupRounds: c.rounds,
  };
  const messages = [
    { role: "system", content: GROUP_RULES },
    { role: "user", content: `Recent group messages, oldest first:\n${transcript}\n\nAnswer the latest message that mentions you, from ${who(c.asker)}.` },
  ];
  return { messagesJson: JSON.stringify(messages), contextJson: JSON.stringify(context) };
}

/** Split the model's raw answer into the text members read and an optional round draft. */
export function parseAnswer(raw: string): { text: string; draft: RoundDraft | null } {
  const fence = /```round-draft\s*([\s\S]*?)```/i;
  const m = raw.match(fence);
  let draft: RoundDraft | null = null;
  if (m) {
    try {
      draft = checkDraft(JSON.parse(m[1]));
    } catch {
      draft = null;
    }
  }
  const text = raw.replace(fence, "").trim().slice(0, MAX_TEXT);
  return { text, draft };
}

/** What the on-device private-information scanner found in a text (mirrors Rust `TextScan`,
 *  src-tauri/src/pii_scan.rs). */
export interface DisclosureScan {
  scanner: string;
  passed: boolean;
  found: { class: string; label: string; count: number }[];
  /** The text with every finding replaced by its class, e.g. `<email>`. */
  redacted: string;
  coverage: string;
}

/** What the member sees on the send card before anything reaches the group (V6). */
export interface SendPreview {
  groupId: string;
  /** How many other members will receive it (null when the roster was not read). */
  recipients: number | null;
  text: string;
  draft: RoundDraft | null;
  /** The round draft's lines exactly as they will be posted (empty when there is no draft). */
  draftLines?: string[];
  read: string[];
  scan: DisclosureScan;
}

/** The member's decision on the send card. `text` is what to post: the answer as shown, the
 *  scanner's redaction, or the member's own edit. */
export type SendDecision = { post: true; text: string } | { post: false };

export interface HermesDeps {
  inferLocal(messagesJson: string, contextJson: string): Promise<string>;
  /** The on-device scanner (`pii_scan_text`). It runs on the answer and again on whatever text the
   *  member chooses to post. */
  scan(text: string): Promise<DisclosureScan>;
  /** Show the send card and wait for the member (V6: the @mention alone never posts). */
  confirm(preview: SendPreview): Promise<SendDecision>;
  /** Post the encoded answer to `groupId` (a structured body this module built). */
  send(groupId: string, body: string): Promise<void>;
}

export type HermesOutcome =
  | { kind: "posted"; body: string }
  | { kind: "no-answer"; reason: string }
  | { kind: "held"; reason: string }
  | { kind: "not-sent"; reason: string; body: string };

/** Members who receive a group message: the roster without the asker. */
export function recipientsOf(c: GroupContext): number | null {
  if (c.roster.length === 0) return null;
  const me = c.asker.toLowerCase();
  return c.roster.filter((r) => r.address.toLowerCase() !== me).length;
}

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * Ask the local model for a group answer to the message with ref `to`, show it to the member on a
 * send card with the private-information scan, and post it only when the member chooses to (V6,
 * owner decision 2026-11-16). Never throws.
 *
 * - The model's answer, and the exact body it would be posted as (round draft included), are
 *   scanned on this device before the card opens. If the scanner cannot run, or the draft holds a
 *   finding, nothing is posted.
 * - Whatever the member chooses to post is scanned again, and it is posted only if the scan
 *   passes: a finding blocks the send whatever the card showed (the member can post the
 *   redaction or edit the text).
 * - Declining, or closing the card, posts nothing.
 */
export async function answerInGroup(groupId: string, to: string, c: GroupContext, deps: HermesDeps): Promise<HermesOutcome> {
  const req = buildRequest(c);
  let raw: string;
  try {
    raw = await deps.inferLocal(req.messagesJson, req.contextJson);
  } catch (e) {
    return { kind: "no-answer", reason: errText(e) };
  }
  const { text, draft } = parseAnswer(raw ?? "");
  if (!text && !draft) return { kind: "no-answer", reason: "the local model returned an empty answer" };
  const shown = text || "Here is a round draft for the owners and admins to review.";
  const read = readList(c);
  // Codex review of #730: the gate scans the exact body that would be posted (the round draft
  // and the lines Hermes adds included), not only the prose. The prose is also scanned alone so
  // the card can offer its redaction.
  const shownBody = encodeHermes({ to, text: shown, read, draft });
  let scan: DisclosureScan;
  let whole: DisclosureScan;
  try {
    scan = await deps.scan(shown);
    whole = await deps.scan(shownBody);
  } catch (e) {
    return { kind: "held", reason: `the private-information check could not run (${errText(e)}), so nothing was posted` };
  }
  if (scan.passed && !whole.passed) {
    return { kind: "held", reason: `the round draft or the lines Hermes adds hold ${whole.found.map((f) => f.label).join(", ")}, so nothing was posted` };
  }
  const decision = await deps
    .confirm({ groupId, recipients: recipientsOf(c), text: shown, draft, draftLines: draft ? draftLines(draft) : [], read, scan })
    .catch(() => ({ post: false }) as SendDecision);
  if (!decision.post) return { kind: "held", reason: "you chose not to post it" };
  const final = decision.text.trim().slice(0, MAX_TEXT);
  if (!final) return { kind: "held", reason: "the answer was empty, so nothing was posted" };
  // What is posted is exactly the body scanned here.
  const body = encodeHermes({ to, text: final, read, draft });
  let check: DisclosureScan;
  try {
    check = body === shownBody ? whole : await deps.scan(body);
  } catch (e) {
    return { kind: "held", reason: `the private-information check could not run (${errText(e)}), so nothing was posted` };
  }
  if (!check.passed) {
    return { kind: "held", reason: `the text still holds ${check.found.map((f) => f.label).join(", ")}, so nothing was posted` };
  }
  try {
    await deps.send(groupId, body);
    return { kind: "posted", body };
  } catch (e) {
    return { kind: "not-sent", reason: errText(e), body };
  }
}
