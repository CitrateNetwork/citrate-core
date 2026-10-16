// =====================================================================
// HUP-S10.4 — the journal bundle: the plaintext that the desktop app seals into
// an encrypted export file (Rust `journal_export`), and the strict parser +
// non-destructive merge used on import.
//
// The plaintext exists only in memory: it is handed to the Rust command, sealed
// there, and only ciphertext is written to disk.
// =====================================================================
import type { JournalPage } from "../shell/state";

export const BUNDLE_FORMAT = "citrate-journal-bundle";
export const BUNDLE_VERSION = 1;

/** A bundle that cannot be imported. The message is safe to show the member. */
export class BundleError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "BundleError";
  }
}

/** Serialize the journal for sealing. `exportedAt` is passed in (ISO string). */
export function buildBundle(pages: JournalPage[], exportedAt: string): string {
  return JSON.stringify({
    format: BUNDLE_FORMAT,
    version: BUNDLE_VERSION,
    exportedAt,
    pages: pages.map((p) => ({ id: p.id, title: p.title, kind: p.kind, pinned: p.pinned, blocks: p.blocks.slice() })),
  });
}

function asPage(v: unknown, i: number): JournalPage {
  const bad = (why: string) => new BundleError(`page ${i + 1} in the file is malformed (${why})`);
  if (typeof v !== "object" || v === null) throw bad("not an object");
  const o = v as Record<string, unknown>;
  if (typeof o.id !== "string" || o.id === "") throw bad("missing id");
  if (typeof o.title !== "string") throw bad("missing title");
  if (o.kind !== "daily" && o.kind !== "page") throw bad("unknown kind");
  if (typeof o.pinned !== "boolean") throw bad("missing pinned flag");
  if (!Array.isArray(o.blocks) || !o.blocks.every((b) => typeof b === "string")) throw bad("blocks must be text");
  return { id: o.id, title: o.title, kind: o.kind, pinned: o.pinned, blocks: (o.blocks as string[]).slice() };
}

/** Parse a decrypted bundle. Throws {@link BundleError} on anything unexpected. */
export function parseBundle(json: string): JournalPage[] {
  let obj: unknown;
  try {
    obj = JSON.parse(json);
  } catch {
    throw new BundleError("the decrypted file is not a journal bundle");
  }
  if (typeof obj !== "object" || obj === null) throw new BundleError("the decrypted file is not a journal bundle");
  const o = obj as Record<string, unknown>;
  if (o.format !== BUNDLE_FORMAT) throw new BundleError("this file is not a Citrate journal export");
  if (o.version !== BUNDLE_VERSION) throw new BundleError(`journal bundle version ${String(o.version)} is not supported by this app`);
  if (!Array.isArray(o.pages)) throw new BundleError("the journal bundle has no pages list");
  return o.pages.map(asPage);
}

function samePage(a: JournalPage, b: JournalPage): boolean {
  return a.title === b.title && a.kind === b.kind && a.blocks.length === b.blocks.length && a.blocks.every((x, i) => x === b.blocks[i]);
}

/**
 * Merge imported pages into the local journal without overwriting anything:
 * - a page whose id is new here is added (unpinned);
 * - an identical page is skipped;
 * - a page whose id exists with different content is added as a separate named
 *   page "<title> (imported)" so the local version is kept and one daily entry
 *   per day still holds.
 */
export function mergeImported(
  existing: JournalPage[],
  imported: JournalPage[],
): { pages: JournalPage[]; added: number; unchanged: number; copied: number } {
  const pages = existing.slice();
  const ids = new Set(pages.map((p) => p.id));
  let added = 0;
  let unchanged = 0;
  let copied = 0;
  for (const imp of imported) {
    const local = pages.find((p) => p.id === imp.id);
    if (!local) {
      pages.push({ ...imp, pinned: false, blocks: imp.blocks.slice() });
      ids.add(imp.id);
      added++;
      continue;
    }
    if (samePage(local, imp)) {
      unchanged++;
      continue;
    }
    let n = 1;
    let id = `${imp.id}-imported`;
    while (ids.has(id)) id = `${imp.id}-imported-${++n}`;
    ids.add(id);
    pages.push({ id, title: `${imp.title} (imported)`, kind: "page", pinned: false, blocks: imp.blocks.slice() });
    copied++;
  }
  return { pages, added, unchanged, copied };
}
