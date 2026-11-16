// =====================================================================
// citrate-core — Hermes in a group (0.5.4 FL design, board 02; agentic chat bar "Shared spaces").
//
// Hermes answers in a group only when a member mentions it, and only on that member's own device:
// the member's app runs the local model with the GROUP'S context alone (its roster and roles, the
// cluster's live peer count, the recent group messages, and whether group rounds are live), then
// posts the answer to the group as a `chermes1` message (groupEnvelope.ts). Nothing from the
// member's private chats, memory, wallet or sealed worker policies is given to the model, so none
// of it can reach the group. Other members' devices never run Hermes for someone else's mention.
//
// Data sources (Rule 7): the comms-member-daemon roster and messages, the cluster daemon's status,
// the round-ledger reading (fl/groupRoundsLedger.ts), and the local model (`ai_chat_local`, the
// bundled llama-server; the webview supplies messages only, never an endpoint). If the local model
// does not answer, nothing is posted and the member is told why.
// =====================================================================
import type { GroupMember, GroupMessage } from "../../bridge/domains";
import { checkDraft, encodeHermes, MAX_TEXT, threadModel, type RoundDraft } from "./groupEnvelope";
import { roleWord } from "./memberRoles";

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

export interface HermesDeps {
  inferLocal(messagesJson: string, contextJson: string): Promise<string>;
  send(groupId: string, body: string): Promise<void>;
}

export type HermesOutcome =
  | { kind: "posted"; body: string }
  | { kind: "no-answer"; reason: string }
  | { kind: "not-sent"; reason: string; body: string };

/**
 * Ask the local model for a group answer to the message with ref `to`, then post it to the group.
 * Never throws: a model failure posts nothing; a send failure says the answer was not sent.
 */
export async function answerInGroup(groupId: string, to: string, c: GroupContext, deps: HermesDeps): Promise<HermesOutcome> {
  const req = buildRequest(c);
  let raw: string;
  try {
    raw = await deps.inferLocal(req.messagesJson, req.contextJson);
  } catch (e) {
    return { kind: "no-answer", reason: e instanceof Error ? e.message : String(e) };
  }
  const { text, draft } = parseAnswer(raw ?? "");
  if (!text && !draft) return { kind: "no-answer", reason: "the local model returned an empty answer" };
  const body = encodeHermes({ to, text: text || "Here is a round draft for the owners and admins to review.", read: readList(c), draft });
  try {
    await deps.send(groupId, body);
    return { kind: "posted", body };
  } catch (e) {
    return { kind: "not-sent", reason: e instanceof Error ? e.message : String(e), body };
  }
}
