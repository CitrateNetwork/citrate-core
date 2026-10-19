// =====================================================================
// citrate-core — Backup and restore (K4). Export Files and Memory to a folder the member picks,
// and restore from one. Composed into Node › Storage; also routed today as the "Backup" tab
// under Files.
//
// Data sources (Rule 7): everything here is done by Rust (`storage_backup_export`,
// `storage_backup_verify`, `storage_backup_restore`); progress comes from the
// `storage://backup-progress` event. The surface never reads or writes the stores itself.
//
// Verifiable: a backup folder carries `citrate-backup.json`, a manifest with the size and SHA-256
// of every file. Restore first checks the folder here (the member sees the result), and Rust
// checks it again and REFUSES on any mismatch before touching anything.
// =====================================================================
import { useState } from "react";
import { SurfaceProps } from "./shared";
import { bridge } from "../bridge";
import { storageSlice, exportBackup, checkBackup, restoreBackup } from "../shell/slices/storage";
import type { BackupPart } from "../bridge/domains";
import { describeMismatch, humanBytes, progressPct, progressText } from "./storageTruth";

const PART_LABEL: Record<BackupPart, string> = { files: "Files", memory: "Memory" };

/** Ask the OS for a folder (Tauri dialog). Null when cancelled. */
async function pickFolder(title: string): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({ directory: true, multiple: false, title });
  return typeof picked === "string" ? picked : null;
}

function ProgressBar() {
  const st = storageSlice.use();
  if (!st.backupOp || st.backupOp === "verify") return null;
  const pct = progressPct(st.backupProgress);
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }} role="status" aria-live="polite">
      <div style={{ height: 6, borderRadius: 999, background: "var(--srf-inset)", overflow: "hidden" }}>
        <div style={{ height: "100%", width: `${pct ?? 0}%`, background: "var(--accent)", transition: "width 200ms" }}></div>
      </div>
      <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
        {st.backupOp === "export" ? "Exporting" : "Restoring"} · {progressText(st.backupProgress)}
        {pct != null ? ` · ${pct}%` : ""}
      </span>
      {st.backupProgress?.item && st.backupProgress.phase !== "done" && (
        <span className="mono" style={{ fontSize: 10, color: "var(--tx-4, var(--tx-3))", wordBreak: "break-all" }}>{st.backupProgress.item}</span>
      )}
    </div>
  );
}

function PartPicker({ parts, setParts, allowed }: { parts: BackupPart[]; setParts: (p: BackupPart[]) => void; allowed: BackupPart[] }) {
  const toggle = (p: BackupPart) => setParts(parts.includes(p) ? parts.filter((x) => x !== p) : [...parts, p].sort());
  return (
    <span style={{ display: "flex", gap: 14 }}>
      {(["files", "memory"] as BackupPart[]).map((p) => (
        <label key={p} style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 12.5, opacity: allowed.includes(p) ? 1 : 0.5 }}>
          <input type="checkbox" checked={parts.includes(p)} disabled={!allowed.includes(p)} onChange={() => toggle(p)} />
          {PART_LABEL[p]}
        </label>
      ))}
    </span>
  );
}

export function StorageBackup({ store }: SurfaceProps) {
  const st = storageSlice.use();
  const [exportParts, setExportParts] = useState<BackupPart[]>(["files", "memory"]);
  const [restoreParts, setRestoreParts] = useState<BackupPart[]>([]);
  const busy = st.backupOp !== null;
  const desktop = bridge.mode === "tauri";
  const check = st.restoreCheck;

  const onExport = async () => {
    if (!desktop) return;
    try {
      const dest = await pickFolder("Choose where to save the backup");
      if (dest) await exportBackup(dest, exportParts);
    } catch (e) {
      store.toast("Couldn't open the folder picker (" + (e instanceof Error ? e.message : String(e)) + ")");
    }
  };

  const onChooseBackup = async () => {
    if (!desktop) return;
    try {
      const folder = await pickFolder("Choose a citrate-backup folder");
      if (!folder) return;
      await checkBackup(folder);
      const c = storageSlice.get().restoreCheck;
      setRestoreParts(c?.ok ? [...c.parts] : []);
    } catch (e) {
      store.toast("Couldn't open the folder picker (" + (e instanceof Error ? e.message : String(e)) + ")");
    }
  };

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, maxWidth: 960 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Backup</span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 10, color: "var(--tx-3)" }}>
          files + memory · to a folder you choose
        </span>
      </div>

      {!desktop && (
        <p role="note" style={{ fontSize: 12.5, color: "var(--tx-3)", margin: 0 }}>
          Backup and restore need the desktop app: the web preview has no local file or memory store.
        </p>
      )}

      {/* export */}
      <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
        <span className="eyebrow">Export a backup</span>
        <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>
          Copies the files kept on this node and your memory graph into a new dated folder, with a manifest that lists
          the SHA-256 of every file. The memory daemon pauses while its store is copied and starts again after. The
          memory store isn&apos;t encrypted by the app, so keep the backup somewhere private.
        </p>
        <PartPicker parts={exportParts} setParts={setExportParts} allowed={["files", "memory"]} />
        <span>
          <button className="btn btn-primary btn-sm" onClick={() => void onExport()} disabled={!desktop || busy || exportParts.length === 0}>
            {st.backupOp === "export" ? "Exporting…" : "Export to a folder…"}
          </button>
        </span>
        {st.backupOp === "export" && <ProgressBar />}
        {st.lastExport && (
          <div data-testid="export-report" style={{ display: "flex", flexDirection: "column", gap: 4, borderTop: "1px solid var(--line-1)", paddingTop: 10 }}>
            <span style={{ fontSize: 13, color: "var(--ok)" }}>Backup written and checked against its manifest.</span>
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)", wordBreak: "break-all" }}>{st.lastExport.folder}</span>
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
              {st.lastExport.files} file{st.lastExport.files === 1 ? "" : "s"} · {st.lastExport.memoryFiles} memory store file
              {st.lastExport.memoryFiles === 1 ? "" : "s"} · {humanBytes(st.lastExport.totalBytes)}
            </span>
            <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", wordBreak: "break-all" }}>manifest digest · {st.lastExport.entriesSha256}</span>
            {st.lastExport.skipped.map((s) => (
              <span key={s.item} className="mono" style={{ fontSize: 10, color: "var(--warn, var(--tx-3))" }}>
                skipped {s.item}: {s.reason}
              </span>
            ))}
            {st.lastExport.notes.map((n) => (
              <span key={n} className="mono" style={{ fontSize: 10, color: "var(--warn, var(--tx-3))" }}>
                {n}
              </span>
            ))}
          </div>
        )}
      </div>

      {/* restore */}
      <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
        <span className="eyebrow">Restore from a backup</span>
        <p style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>
          Choose a citrate-backup folder. It&apos;s checked file by file against its manifest first; a backup that
          doesn&apos;t match is refused and nothing changes. Restoring Memory keeps your current memory store beside the
          restored one instead of deleting it. Restored files are pinned on this node (a local pin, no bond).
        </p>
        <span>
          <button className="btn btn-secondary btn-sm" onClick={() => void onChooseBackup()} disabled={!desktop || busy}>
            {st.backupOp === "verify" ? "Checking…" : "Choose a backup folder…"}
          </button>
        </span>

        {check && check.ok && (
          <div data-testid="restore-check-ok" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <span style={{ fontSize: 13, color: "var(--ok)" }}>
              Matches its manifest: {check.fileCount} file{check.fileCount === 1 ? "" : "s"}, {humanBytes(check.totalBytes)}
              {check.createdAt ? `, made ${new Date(check.createdAt * 1000).toLocaleString()}` : ""}.
            </span>
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", wordBreak: "break-all" }}>{check.folder}</span>
            <PartPicker parts={restoreParts} setParts={setRestoreParts} allowed={check.parts} />
            <span>
              <button
                className="btn btn-primary btn-sm"
                disabled={busy || restoreParts.length === 0}
                onClick={() => void restoreBackup(check.folder, restoreParts)}
              >
                {st.backupOp === "restore" ? "Restoring…" : "Restore " + restoreParts.map((p) => PART_LABEL[p]).join(" and ")}
              </button>
            </span>
          </div>
        )}

        {check && !check.ok && (
          <div role="alert" data-testid="restore-check-refused" style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <span style={{ fontSize: 13, color: "var(--danger)" }}>Restore refused: this folder doesn&apos;t match its manifest.</span>
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", wordBreak: "break-all" }}>{check.folder}</span>
            {check.problem && (
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>
                {check.problem}
              </span>
            )}
            {check.mismatches.map((m) => (
              <span key={m.kind + m.path} className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>
                {describeMismatch(m)}
              </span>
            ))}
          </div>
        )}

        {st.backupOp === "restore" && <ProgressBar />}

        {st.lastRestore && (
          <div data-testid="restore-report" style={{ display: "flex", flexDirection: "column", gap: 4, borderTop: "1px solid var(--line-1)", paddingTop: 10 }}>
            <span style={{ fontSize: 13, color: "var(--ok)" }}>Restored and checked.</span>
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
              {st.lastRestore.filesRestored} file{st.lastRestore.filesRestored === 1 ? "" : "s"} · {st.lastRestore.memoryFilesRestored} memory store file
              {st.lastRestore.memoryFilesRestored === 1 ? "" : "s"}
            </span>
            {st.lastRestore.previousMemoryKeptAt && (
              <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", wordBreak: "break-all" }}>
                your previous memory store is kept at {st.lastRestore.previousMemoryKeptAt}
              </span>
            )}
            {st.lastRestore.cidsChanged.map((c) => (
              <span key={c.expected} className="mono" style={{ fontSize: 10, color: "var(--warn, var(--tx-3))", wordBreak: "break-all" }}>
                {c.expected} came back as {c.actual} (same bytes, different CID version)
              </span>
            ))}
            {st.lastRestore.notes.map((n) => (
              <span key={n} className="mono" style={{ fontSize: 10, color: "var(--warn, var(--tx-3))" }}>
                {n}
              </span>
            ))}
          </div>
        )}
      </div>

      {st.backupError && (
        <div role="alert" className="surface" style={{ padding: 14, borderColor: "var(--danger)" }}>
          <span style={{ display: "block", fontSize: 13, color: "var(--danger)" }}>
            {/refus|does not match/i.test(st.backupError) ? "Restore refused." : "That didn't work."}
          </span>
          <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-2)", marginTop: 4 }}>{st.backupError}</span>
        </div>
      )}
    </div>
  );
}
