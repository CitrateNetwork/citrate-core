// 0.5.4 — export Hermes conversations, encrypted (ADR P-11: retention, export and delete).
//
// Nothing new is invented for the export. The conversations become journal pages, and the
// journal's existing encrypted export seals them (src/journal/encryptedExport.ts, which hands
// them to the Rust journal_export_encrypted command: Argon2id with the vault's parameters, then
// AES-256-GCM). The file is written only as ciphertext, to the path the member picks. The
// Journal's "Import" opens it again as pages, and never overwrites a page that is already there.
import type { JournalPage } from "../shell/state";
import type { ChatThread } from "../shell/slices/chatHistory";

const day = (ts: number) => new Date(ts).toISOString().slice(0, 10);

/** One journal page per conversation: a header line, then one block per message. */
export function threadsAsJournalPages(threads: ChatThread[]): JournalPage[] {
  return threads.map((t) => ({
    id: "hermes-" + t.id,
    title: "Hermes: " + t.title,
    kind: "page" as const,
    pinned: false,
    blocks: [
      `Hermes conversation, ${day(t.createdAt)} to ${day(t.updatedAt)}, ${t.msgs.length} messages.`,
      ...t.msgs.map((m) => `${m.who}: ${m.text}` + (m.error ? ` (did not finish: ${m.error})` : "")),
    ],
  }));
}
