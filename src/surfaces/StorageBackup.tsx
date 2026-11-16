// =====================================================================
// citrate-core — Backup and restore (K4). Export Files and Memory to a folder the member picks,
// and restore from one. Composed into Node › Storage; also routed today as the "Backup" tab
// under Files.
//
// Data sources (Rule 7): everything here is done by Rust (`storage_backup_export`,
// `storage_backup_verify`, `storage_backup_restore`); progress comes from the
// `storage://backup-progress` event. The surface never reads or writes the stores itself.
//
// Verifiable: a backup carries `citrate-backup.json`, a manifest with the size and SHA-256 of
// every file. Restore first checks the backup here (the member sees the result), and Rust checks
// it again and REFUSES on any mismatch before touching anything.
//
// STO-2: a backup is one file sealed under the member's passphrase by default (Argon2id +
// AES-256-GCM in Rust; the passphrase is never stored). The plain folder is written only when the
// member ticks "without encryption" and sees the warning.
// =====================================================================
import { useState } from "react";
import { SurfaceProps } from "./shared";
import { bridge } from "../bridge";
import { storageSlice, exportBackup, checkBackup, restoreBackup } from "../shell/slices/storage";
import type { BackupPart } from "../bridge/domains";
import {
  describeMismatch,
  exportProtection,
  humanBytes,
  MIN_BACKUP_PASSPHRASE,
  progressPct,
  progressText,
  SEALED_EXT,
} from "./storageTruth";

const PART_LABEL: Record<BackupPart, string> = { files: "Files", memory: "Memory" };

const isPart = (item: string): item is BackupPart => item === "files" || item === "memory";
const partList = (parts: BackupPart[]) => parts.map((p) => PART_LABEL[p]).join(" and ");

/** Ask the OS for a folder (Tauri dialog). Null when cancelled. */
async function pickFolder(title: string): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({ directory: true, multiple: false, title });
  return typeof picked === "string" ? picked : null;
}

/** Ask the OS for an encrypted backup file (Tauri dialog). Null when cancelled. */
async function pickSealedFile(title: string): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    directory: false,
    multiple: false,
    title,
    filters: [{ name: "Citrate encrypted backup", extensions: [SEALED_EXT] }],
  });
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
  const [pass, setPass] = useState("");
  const [confirm, setConfirm] = useState("");
  const [plaintext, setPlaintext] = useState(false);
  const [sealedPath, setSealedPath] = useState<string | null>(null);
  const [restorePass, setRestorePass] = useState("");
  const choice = exportProtection(pass, confirm, plaintext);
  const busy = st.backupOp !== null;
  const desktop = bridge.mode === "tauri";
  const check = st.restoreCheck;

  const onExport = async () => {
    if (!desktop) return;
    try {
      if (!choice.protection) return;
      const dest = await pickFolder("Choose where to save the backup");
      if (dest) {
        await exportBackup(dest, exportParts, choice.protection);
        setPass("");
        setConfirm("");
      }
    } catch (e) {
      store.toast("Couldn't open the folder picker (" + (e instanceof Error ? e.message : String(e)) + ")");
    }
  };

  const onChooseBackup = async () => {
    if (!desktop) return;
    try {
      const folder = await pickFolder("Choose a citrate-backup folder");
      if (!folder) return;
      setSealedPath(null);
      setRestorePass("");
      await checkBackup(folder);
      const c = storageSlice.get().restoreCheck;
      setRestoreParts(c?.ok ? [...c.parts] : []);
    } catch (e) {
      store.toast("Couldn't open the folder picker (" + (e instanceof Error ? e.message : String(e)) + ")");
    }
  };

  const onChooseSealed = async () => {
    if (!desktop) return;
    try {
      const file = await pickSealedFile("Choose an encrypted backup file");
      if (!file) return;
      setSealedPath(file);
      setRestorePass("");
      setRestoreParts([]);
    } catch (e) {
      store.toast("Couldn't open the file picker (" + (e instanceof Error ? e.message : String(e)) + ")");
    }
  };

  const onOpenSealed = async () => {
    if (!sealedPath || !restorePass) return;
    await checkBackup(sealedPath, restorePass);
    const c = storageSlice.get().restoreCheck;
    setRestoreParts(c?.ok ? [...c.parts] : []);
  };

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, maxWidth: 960 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Backup</span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 10, color: "var(--tx-3)" }}>
          files + memory · encrypted · to a folder you choose
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
          Copies the files kept on this node and your memory graph into one dated backup file, encrypted with a
          passphrase you choose (Argon2id and AES-256-GCM). Inside is a manifest that lists the SHA-256 of every file.
          The memory daemon pauses while its store is copied and starts again after. The passphrase is not saved
          anywhere: without it the backup can&apos;t be opened, by you or by Citrate.
        </p>
        <PartPicker parts={exportParts} setParts={setExportParts} allowed={["files", "memory"]} />
        {!plaintext && (
          <span style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
            <input
              type="password"
              aria-label="Backup passphrase"
              placeholder={`Passphrase (${MIN_BACKUP_PASSPHRASE}+ characters)`}
              value={pass}
              autoComplete="new-password"
              onChange={(e) => setPass(e.target.value)}
              style={{ minWidth: 240 }}
            />
            <input
              type="password"
              aria-label="Repeat the backup passphrase"
              placeholder="Repeat the passphrase"
              value={confirm}
              autoComplete="new-password"
              onChange={(e) => setConfirm(e.target.value)}
              style={{ minWidth: 240 }}
            />
          </span>
        )}
        <label style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 12.5 }}>
          <input type="checkbox" checked={plaintext} onChange={(e) => setPlaintext(e.target.checked)} />
          Export without encryption
        </label>
        {plaintext && (
          <p role="alert" data-testid="plaintext-warning" style={{ fontSize: 12.5, lineHeight: 1.6, color: "var(--warn, var(--danger))", margin: 0 }}>
            Not encrypted: the backup is a plain folder, and anyone who can open it can read your memory graph and every
            backed-up file. The manifest is not signed. Only use this for a disk only you can reach.
          </p>
        )}
        {choice.reason && !plaintext && (pass || confirm) && (
          <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>{choice.reason}</span>
        )}
        <span>
          <button
            className="btn btn-primary btn-sm"
            onClick={() => void onExport()}
            disabled={!desktop || busy || exportParts.length === 0 || !choice.protection}
          >
            {st.backupOp === "export" ? "Exporting…" : plaintext ? "Export unencrypted to a folder…" : "Export encrypted to a folder…"}
          </button>
        </span>
        {st.backupOp === "export" && <ProgressBar />}
        {st.lastExport && (
          <div data-testid="export-report" style={{ display: "flex", flexDirection: "column", gap: 4, borderTop: "1px solid var(--line-1)", paddingTop: 10 }}>
            <span style={{ fontSize: 13, color: "var(--ok)" }}>
              {st.lastExport.encrypted
                ? "Encrypted backup written. Its contents were checked against the manifest before sealing."
                : "Unencrypted backup written and checked against its manifest."}
            </span>
            <span style={{ fontSize: 12.5 }}>Backed up {partList(st.lastExport.parts)}.</span>
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)", wordBreak: "break-all" }}>{st.lastExport.folder}</span>
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
              {[
                st.lastExport.parts.includes("files") ? `${st.lastExport.files} file${st.lastExport.files === 1 ? "" : "s"}` : null,
                st.lastExport.parts.includes("memory")
                  ? `${st.lastExport.memoryFiles} memory store file${st.lastExport.memoryFiles === 1 ? "" : "s"}`
                  : null,
                humanBytes(st.lastExport.totalBytes),
              ]
                .filter(Boolean)
                .join(" · ")}
            </span>
            <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", wordBreak: "break-all" }}>manifest digest · {st.lastExport.entriesSha256}</span>
            {st.lastExport.skipped.map((s) => (
              <span key={s.item} className="mono" style={{ fontSize: 10, color: "var(--warn, var(--tx-3))" }}>
                {isPart(s.item) ? `${PART_LABEL[s.item]} not backed up: ${s.reason}` : `skipped ${s.item}: ${s.reason}`}
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
          Choose an encrypted backup file (and enter its passphrase) or a plain citrate-backup folder. It&apos;s checked
          file by file against its manifest first; a wrong passphrase or a backup that doesn&apos;t match is refused and
          nothing changes. Restoring Memory keeps your current memory store beside the
          restored one instead of deleting it. Restored files are pinned on this node (a local pin, no bond).
        </p>
        <span style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
          <button className="btn btn-secondary btn-sm" onClick={() => void onChooseSealed()} disabled={!desktop || busy}>
            Choose an encrypted backup…
          </button>
          <button className="btn btn-secondary btn-sm" onClick={() => void onChooseBackup()} disabled={!desktop || busy}>
            {st.backupOp === "verify" && !sealedPath ? "Checking…" : "Choose a plain backup folder…"}
          </button>
        </span>
        {sealedPath && (
          <span data-testid="sealed-open" style={{ display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center" }}>
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", wordBreak: "break-all" }}>{sealedPath}</span>
            <input
              type="password"
              aria-label="Passphrase of this backup"
              placeholder="Passphrase"
              value={restorePass}
              autoComplete="current-password"
              onChange={(e) => setRestorePass(e.target.value)}
              style={{ minWidth: 220 }}
            />
            <button className="btn btn-secondary btn-sm" onClick={() => void onOpenSealed()} disabled={busy || !restorePass}>
              {st.backupOp === "verify" ? "Opening…" : "Open and check"}
            </button>
          </span>
        )}

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
                onClick={() => void restoreBackup(check.folder, restoreParts, sealedPath === check.folder ? restorePass : undefined)}
              >
                {st.backupOp === "restore" ? "Restoring…" : "Restore " + restoreParts.map((p) => PART_LABEL[p]).join(" and ")}
              </button>
            </span>
          </div>
        )}

        {check && !check.ok && (
          <div role="alert" data-testid="restore-check-refused" style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <span style={{ fontSize: 13, color: "var(--danger)" }}>
              {check.problem && /passphrase/i.test(check.problem)
                ? "Restore refused: that passphrase doesn't open this backup, or the file is damaged."
                : "Restore refused: this backup doesn't match its manifest."}
            </span>
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
            <span style={{ fontSize: 13, color: "var(--ok)" }}>Restored {partList(st.lastRestore.parts)} and checked.</span>
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
              {[
                st.lastRestore.parts.includes("files")
                  ? `${st.lastRestore.filesRestored} file${st.lastRestore.filesRestored === 1 ? "" : "s"}`
                  : null,
                st.lastRestore.parts.includes("memory")
                  ? `${st.lastRestore.memoryFilesRestored} memory store file${st.lastRestore.memoryFilesRestored === 1 ? "" : "s"}`
                  : null,
              ]
                .filter(Boolean)
                .join(" · ")}
            </span>
            {st.lastRestore.skipped.map((s) => (
              <span key={s.item} className="mono" style={{ fontSize: 10, color: "var(--warn, var(--tx-3))" }}>
                {isPart(s.item) ? PART_LABEL[s.item] : s.item} not restored: {s.reason}
              </span>
            ))}
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
