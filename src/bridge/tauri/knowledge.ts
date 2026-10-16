// HUP-S3.1 — first-run knowledge-corpus import (TAURI). Subscribes to the Rust progress event for
// the duration of the import and always unlistens, even when the import rejects.
import { listen } from "@tauri-apps/api/event";
import { invoke } from "./invoke";
import type { KnowledgeImportLine, KnowledgeImportReport } from "../domains";

/** Mirrors `knowledge_import::PROGRESS_EVENT` in Rust. */
export const KNOWLEDGE_PROGRESS_EVENT = "memory://knowledge-import-progress";

export async function tauriImportKnowledge(
  onProgress?: (line: KnowledgeImportLine) => void,
): Promise<KnowledgeImportReport> {
  const unlisten = onProgress
    ? await listen<KnowledgeImportLine>(KNOWLEDGE_PROGRESS_EVENT, (ev) => {
        if (ev.payload && typeof ev.payload === "object" && "event" in ev.payload) onProgress(ev.payload);
      })
    : null;
  try {
    return await invoke<KnowledgeImportReport>("memory_import_knowledge");
  } finally {
    unlisten?.();
  }
}
