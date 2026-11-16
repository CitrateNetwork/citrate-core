// =====================================================================
// citrate-core: what the member attaches to a Hermes message with @ (FL design board 01, #690)
//
// Every item comes from a real source on this device:
//  - datasets: core's assembled round training files (fl_trajectories.rs trajectories_datasets_list:
//    name, size, example count, sha256; never their lines);
//  - groups: the comms daemon's group list (bridge.groups.list: name, kind, member count);
//  - journal pages: the member's own journal (AppState.jPages).
// What Hermes receives is a short block core writes in front of the message: a dataset or a group
// by its description only, a journal page with its text (the member's own words, clipped).
// =====================================================================
import type { FlDatasetFile, Group } from "../bridge/domains";
import type { JournalPage } from "../shell/state";

export type AttachmentKind = "dataset" | "group" | "journal";

export interface ChatAttachment {
  kind: AttachmentKind;
  id: string;
  /** The chip's words. */
  label: string;
  /** What Hermes is told about it (one line, or the page text for a journal page). */
  context: string;
}

/** Most items attached to one message. */
export const MAX_ATTACHMENTS = 6;
/** Longest journal text sent with a page (characters). */
export const MAX_JOURNAL_CHARS = 2000;

const mb = (n: number) => (n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1000))} KB`);
const day = (ms: number) => new Date(ms).toISOString().slice(0, 10);

export function datasetAttachment(d: FlDatasetFile): ChatAttachment {
  return {
    kind: "dataset",
    id: "dataset:" + d.name,
    label: `Dataset · ${d.examples} conversations, ${day(d.builtMs)}`,
    context: `Dataset ${d.name} on this device: ${d.examples} verified Hermes conversations, ${mb(d.bytes)}, built ${day(d.builtMs)}, sha256 ${d.sha256}. Its lines are not included; it never leaves this device unless the member adds it to a round.`,
  };
}

export function groupAttachment(g: Group): ChatAttachment {
  const n = g.members.length;
  return {
    kind: "group",
    id: "group:" + g.id,
    label: `Group · ${g.name}`,
    context: `Group "${g.name}" (id ${g.id}, a ${g.kind}, ${n} ${n === 1 ? "member" : "members"}). Its messages are not included.`,
  };
}

export function journalAttachment(p: JournalPage): ChatAttachment {
  const text = p.blocks.join("\n").trim();
  const clipped = text.length > MAX_JOURNAL_CHARS ? text.slice(0, MAX_JOURNAL_CHARS - 1) + "…" : text;
  return {
    kind: "journal",
    id: "journal:" + p.id,
    label: `Journal · ${p.title || "Untitled page"}`,
    context: `Journal page "${p.title || "Untitled page"}" (the member's own notes):\n${clipped || "(empty page)"}`,
  };
}

/** Items whose label matches what follows the @ (case-insensitive), at most `limit`. */
export function matchAttachments(all: ChatAttachment[], query: string, limit = 8): ChatAttachment[] {
  const q = query.trim().toLowerCase();
  return all.filter((a) => !q || a.label.toLowerCase().includes(q)).slice(0, limit);
}

/** The @word being typed at the end of the text, or null (an @ starts a word, e.g. not "a@b"). */
export function mentionAt(text: string): { start: number; query: string } | null {
  const m = /(^|\s)@([^\s@]{0,40})$/.exec(text);
  if (!m) return null;
  return { start: text.length - m[2].length - 1, query: m[2] };
}

/**
 * The block core puts in front of the member's message for Hermes: the attached items, and the
 * kinds of context the member turned off for this chat. Empty when there is neither.
 */
export function contextPreamble(attachments: readonly ChatAttachment[], offTitles: readonly string[]): string {
  const parts: string[] = [];
  if (attachments.length > 0) {
    parts.push("The member attached this for context (written by Citrate Core):\n" + attachments.map((a) => "- " + a.context).join("\n"));
  }
  if (offTitles.length > 0) {
    parts.push(`The member turned off for this chat: ${offTitles.join(", ")}. Core refuses those tools here; do not call them.`);
  }
  return parts.length ? parts.join("\n\n") + "\n\nThe member's message:\n" : "";
}
